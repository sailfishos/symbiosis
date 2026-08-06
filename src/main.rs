// Copyright (c) 2026 Jolla Mobile Ltd

//! TOH daemon for detecting presence, fetching and publishing info, and starting and stopping
//! services.

use argh::FromArgs;
use log::{debug, info, warn, LevelFilter};
use std::error::Error;
use std::time::Duration;
use symbiosis::{
    back_cover::{BackCover, DetectionError, PowerDown, Variant, WaitDisconnect},
    dbus::server::Toh,
    toh::{Detect, IsPresent},
};
use systemd_journal_logger::JournalLog;
use tokio::{select, time::sleep};
use zbus::{fdo::ObjectManager, Connection};

const SERVICE_NAME: &str = "org.sailfishos.tohd1";
const SERVICE_PATH: &str = "/org/sailfishos/tohd1";
const TOH_PATH: &str = "/org/sailfishos/tohd1/toh";

trait BackCoverDetect: WaitDisconnect + Detect + PowerDown {
    // Rust 1.86.0 gets rid of this
    fn cast_to_wait_disconnect(self: Box<Self>) -> Box<dyn WaitDisconnect>;
}

impl<T: WaitDisconnect + Detect + PowerDown + 'static> BackCoverDetect for T {
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
    let mut unsupported_message_logged = false;

    // After start, check if the cover is already present so we can skip some of the
    // services from starting. Useful e.g. to avoid changing ambience on boot or service
    // restart.
    let mut toh_already_present = true;

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
                Ok(Variant::Attached(back_cover)) => {
                    if !unsupported_message_logged {
                        info!("Unsupported TOH type connected");
                        unsupported_message_logged = true;
                    }
                    // Since this is fairly unlikely, we retry detection after some time even if the
                    // TOH has not been disconnected.
                    select! {
                        result = back_cover.wait_disconnect() => {
                            result?;
                        }
                        _ = sleep(Duration::from_secs(10)) => {}
                    };
                    None
                }
                Err(error) => {
                    warn!("TOH power up failed: {error}");
                    sleep(Duration::from_secs(10)).await;
                    None
                }
            };
        if let Some(mut back_cover) = detect {
            // Wait a bit after powering up so the chip has a chance to be ready
            sleep(Duration::from_millis(100)).await;
            match back_cover.detect().await {
                Ok(Some(mut info)) => {
                    let back_cover: Box<dyn WaitDisconnect> =
                        if info.leave_power_on.unwrap_or(false) {
                            back_cover.cast_to_wait_disconnect()
                        } else {
                            Box::new(back_cover.power_down_boxed()?)
                        };
                    let units = match info.read_configs() {
                        Err(error) => {
                            warn!("Failed to read config: {error}");
                            None
                        }
                        Ok(Some(configs)) => {
                            let (overrides, units) = configs.split();
                            info.apply_overrides(overrides);
                            Some(units)
                        }
                        Ok(None) => None,
                    };
                    // Publish TOH on D-Bus before starting services
                    let toh = Toh::new(info);
                    object_server.at(TOH_PATH, toh).await?;
                    let units = if let Some(units) = units {
                        // toh_already_present <=> service is starting + TOH is connected
                        Some(units.start_units(toh_already_present).await)
                    } else {
                        None
                    };
                    back_cover.wait_disconnect_boxed().await?;
                    if let Some(units) = units {
                        units.stop_units().await;
                    }
                    object_server.remove::<Toh, _>(TOH_PATH).await?;
                }
                Ok(None) => {
                    // TOH was removed. Let's loop back.
                    info!("TOH was disconnected while detecting it");
                }
                Err(error) => {
                    // Reasons why this might happen include that TOH was not yet properly
                    // connected, or the type might have been misdetected, so it is a good idea to
                    // try again after a while.
                    warn!("Detecting TOH failed: {error:?}");
                }
            }
            sleep(Duration::from_secs(1)).await;
        };
    }
}
