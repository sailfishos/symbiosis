// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! TOH daemon for detecting presence, fetching and publishing info, and starting and stopping
//! services.

use argh::FromArgs;
use log::{debug, info, warn, LevelFilter};
use std::error::Error;
use std::time::Duration;
use symbiosis::{
    back_cover::{
        BackCover, DetectionError, EnableTargetDevices, IdentificationError, PowerDown, Variant,
        WaitDisconnect,
    },
    dbus::server::Toh,
    toh::{Detect, IsPresent},
};
use systemd_journal_logger::JournalLog;
use tokio::{
    join,
    time::{sleep, timeout},
};
use zbus::{fdo::ObjectManager, Connection};

const SERVICE_NAME: &str = "org.sailfishos.tohd1";
const SERVICE_PATH: &str = "/org/sailfishos/tohd1";
const TOH_PATH: &str = "/org/sailfishos/tohd1/toh";

/// Error kinds on TOH detection.
#[derive(Default, PartialEq)]
enum LastErrorKind {
    #[default]
    NoError,
    UnsupportedToh,
    BadlySeated,
    MissingIdResistor,
    IdentificationError,
    ChipReadingError,
    ParsingError,
}

/// Keep count on retries per error kind.
#[derive(Default)]
struct LastError {
    /// Number of reattempts on the same error.
    count: u8,
    /// Last seen error kind.
    kind: LastErrorKind,
}

impl LastError {
    fn at_limit(&self) -> bool {
        let retry_limit = match self.kind {
            LastErrorKind::NoError => {
                // NoError is never at limit.
                return false;
            }
            // Give this a retry chance.
            LastErrorKind::UnsupportedToh => 1,
            // Here we should wait that the user manages to attach TOH properly.
            LastErrorKind::BadlySeated => 99,
            // Either TOH is broken or ID pin does not touch yet.
            LastErrorKind::MissingIdResistor => 49,
            // Otherwise 5 attempts seems like a reasonable number.
            _ => 4,
        };
        self.count >= retry_limit
    }

    fn reset(&mut self) {
        self.update(LastErrorKind::NoError);
    }

    /// Update last error kind and return whether it changed.
    fn update(&mut self, kind: LastErrorKind) -> bool {
        if self.kind == kind {
            self.count = self.count.saturating_add(1);
            false
        } else {
            // If error kind changes, reset count as clearly something is happening.
            self.count = 0;
            self.kind = kind;
            true
        }
    }

    fn update_from_identification_error(&mut self, error: &IdentificationError) -> bool {
        let kind = match error {
            // User removed the back cover. Nothing to see here.
            IdentificationError::Disconnected => LastErrorKind::NoError,
            // User is probably still attaching TOH, so let's try again in a moment.
            IdentificationError::BadlySeated => LastErrorKind::BadlySeated,
            // This could be either that TOH is not yet pressed in fully, or it is defective and ID
            // resistor is not connected.
            IdentificationError::IdResistorNotDetected => LastErrorKind::MissingIdResistor,
            _ => LastErrorKind::IdentificationError,
        };
        self.update(kind)
    }

    fn update_from_detection_error(&mut self, error: &DetectionError) -> bool {
        // These could both happen also when TOH is not that well seated but we don't expect them to
        // alternate when TOH is broken.
        let kind = match error {
            DetectionError::Io(_) => LastErrorKind::ChipReadingError,
            DetectionError::Parse(_) => LastErrorKind::ParsingError,
        };
        self.update(kind)
    }
}

trait BackCoverDetect: WaitDisconnect + Detect + PowerDown + EnableTargetDevices {
    // Rust 1.86.0 gets rid of this
    fn cast_to_wait_disconnect(self: Box<Self>) -> Box<dyn WaitDisconnect>;
}

impl<T: WaitDisconnect + Detect + PowerDown + EnableTargetDevices + 'static> BackCoverDetect for T {
    // Rust 1.86.0 gets rid of this
    fn cast_to_wait_disconnect(self: Box<Self>) -> Box<dyn WaitDisconnect> {
        self
    }
}

#[derive(FromArgs)]
#[argh(
    help_triggers("-h", "--help"),
    description = "Daemon to provide TOH info and start related services."
)]
struct Arguments {
    /// log at debug level.
    #[argh(switch, short = 'd', long = "debug")]
    log_debug: bool,
}

// TODO: Use capabilities, no need to have root access to everything

/// Symbiosis TOH daemon entry point.
///
/// This service creates D-Bus service before it has looked for any TOHs. Users should check
/// ObjectManager interfaces for new paths to avoid race conditions.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    JournalLog::new()?.install()?;

    let args: Arguments = argh::from_env();
    log::set_max_level(if args.log_debug {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    });

    let connection = Connection::system().await?;
    let object_server = connection.object_server();
    object_server.at(SERVICE_PATH, ObjectManager).await?;
    connection
        .request_name(SERVICE_NAME)
        .await
        .map_err(|error| {
            // Rust 1.76.0 would let us use inspect_err instead.
            if matches!(error, zbus::Error::NameTaken) {
                log::error!("Bus name is already taken. Is symbiosis already running?");
            } else {
                log::error!("Error registering name: {error}");
            }
            error
        })?;
    debug!("Connected to D-Bus");

    // After start, check if the cover is already present so we can skip some of the
    // services from starting. Useful e.g. to avoid changing ambience on boot or service
    // restart.
    let mut toh_already_present = true;
    // Keep tab of some types of errors.
    let mut last_error = LastError::default();

    loop {
        let mut back_cover = BackCover::new()?;
        if toh_already_present {
            // Checking (again) if TOH is there. Error cases end up here and usually we loop again
            // back here after TOH has been removed.
            toh_already_present = back_cover.is_present().await?;
        }
        if !toh_already_present {
            debug!("Looking for TOH");
        }
        let back_cover = back_cover.wait_connect().await?;
        debug!("TOH connected");
        let detect: Option<Box<dyn BackCoverDetect<Error = DetectionError>>> =
            match back_cover.power_up().await {
                Ok(Variant::With256BBlocks(back_cover)) => {
                    debug!("Up to 256B block memory chip detected");
                    Some(Box::new(back_cover))
                }
                Ok(Variant::With64kBBlocks(back_cover)) => {
                    debug!("Up to 64k block memory chip detected");
                    Some(Box::new(back_cover))
                }
                Ok(Variant::WithoutMemoryChip(back_cover)) => {
                    debug!("TOH without memory chip detected");
                    Some(Box::new(back_cover))
                }
                Ok(Variant::Attached(_)) => {
                    if last_error.update(LastErrorKind::UnsupportedToh) {
                        info!("Unsupported TOH type connected");
                    }
                    None
                }
                Err(error) => {
                    if last_error.update_from_identification_error(&error) {
                        warn!("TOH power up failed: {error}");
                    } else {
                        debug!("Repeated identification error: {error}");
                    }
                    None
                }
            };
        if let Some(mut back_cover) = detect {
            // Wait a bit after powering up so the chip has a chance to be ready
            sleep(Duration::from_millis(100)).await;
            match back_cover.detect().await {
                Ok(Some(mut info)) => {
                    // Detection succeeded => reset retry count.
                    last_error.reset();

                    let (units, permissions) = match info.read_configs() {
                        Err(error) => {
                            warn!("Failed to read config: {error}");
                            None
                        }
                        Ok(Some(configs)) => {
                            let (overrides, devices, units, permissions) = configs.split();
                            info.apply_overrides(overrides);
                            // Expose configured I²C addresses and load drivers.
                            back_cover.enable_target_devices(devices)?;
                            Some((units, permissions))
                        }
                        Ok(None) => None,
                    }
                    .unzip();

                    // Power down if requested.
                    let back_cover: Box<dyn WaitDisconnect> =
                        if info.leave_power_on.unwrap_or(false) {
                            back_cover.cast_to_wait_disconnect()
                        } else {
                            Box::new(back_cover.power_down_boxed()?)
                        };

                    // Publish TOH on D-Bus before starting services
                    let toh = Toh::new(info, permissions.unwrap_or_default());
                    object_server.at(TOH_PATH, toh).await?;

                    // Wait for disconnect already while starting units
                    let disconnect_future = back_cover.wait_disconnect_boxed();
                    let (disconnect_result, units) = if let Some(units) = units {
                        let (result, units) = join!(
                            disconnect_future,
                            // toh_already_present <=> service is starting + TOH is connected
                            units.start_units(toh_already_present),
                        );
                        (result, Some(units))
                    } else {
                        (disconnect_future.await, None)
                    };

                    // Stop units if something goes wrong before returning result
                    if let Some(units) = units {
                        units.stop_units().await;
                    }
                    // If this returns, the daemon stops and it leaves D-Bus anyway
                    disconnect_result?;
                    object_server.remove::<Toh, _>(TOH_PATH).await?;
                }
                Ok(None) => {
                    // TOH was removed. Let's reset retry count and loop back.
                    info!("TOH was disconnected while detecting it");
                    last_error.reset();
                }
                Err(error) => {
                    if last_error.update_from_detection_error(&error) {
                        warn!("Detecting TOH failed: {error:?}");
                    } else {
                        debug!("Repeated detection error: {error:?}");
                    }
                }
            }
        };
        // Looping around, turn off power and do some checks.
        let back_cover = BackCover::new()?;
        if last_error.at_limit() {
            // Retry count reached, wait that TOH is disconnected before continuing.
            warn!("Retry count reached, waiting for TOH to be disconnected");
            last_error.reset();
            back_cover.wait_disconnect().await?;
            debug!("TOH disconnected, continuing");
        } else {
            // In case there was an error sleep for a second or until TOH is disconnected.
            timeout(Duration::from_secs(1), back_cover.wait_disconnect())
                .await
                .ok()
                .transpose()?;
        }
    }
}
