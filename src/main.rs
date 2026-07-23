// Copyright (c) 2026 Jolla Mobile Ltd

//! TOH daemon.
//!
//! The working name is 'symbiosis'.
//!
//! Currently this is a very minimal implementation, mainly good for checking if a TOH is attached
//! and reading the memory chip contents.

use log::{debug, info, warn, LevelFilter};
use std::env::args;
use std::error::Error;
use std::time::Duration;
use symbiosis::{
    back_cover::{BackCover, DetectionError, Variant, WaitDisconnect},
    dbus::server::Toh,
    toh::Detect,
};
use systemd_journal_logger::JournalLog;
use tokio::time::sleep;
use zbus::{fdo::ObjectManager, Connection};

const SERVICE_NAME: &str = "org.sailfishos.tohd1";
const SERVICE_PATH: &str = "/org/sailfishos/tohd1";
const TOH_PATH: &str = "/org/sailfishos/tohd1/toh";

trait BackCoverDetect: WaitDisconnect + Detect {}

impl<T: WaitDisconnect + Detect> BackCoverDetect for T {}

// TODO: Use capabilities, no need to have root access to everything

/// Symbiosis TOH daemon entry point.
///
/// This service creates D-Bus service before it has looked for any TOHs. Users should check
/// ObjectManager interfaces for new paths to avoid race conditions.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    JournalLog::new()?.install()?;

    let mut log_level = LevelFilter::Info;
    for arg in args().skip(1) {
        match arg.as_str() {
            "--debug" => {
                log_level = LevelFilter::Debug;
            }
            "--trace" => {
                log_level = LevelFilter::Trace;
            }
            arg => {
                Err(format!("Bad argument: {arg}"))?;
            }
        }
    }

    log::set_max_level(log_level);

    let connection = Connection::system().await?;
    let object_server = connection.object_server();
    object_server.at(SERVICE_PATH, ObjectManager).await?;
    connection.request_name(SERVICE_NAME).await?;
    debug!("Connected to D-Bus");

    loop {
        let back_cover = BackCover::new()?;
        debug!("Looking for TOH");
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
                    // TODO: Implement
                    back_cover.wait_disconnect().await?;
                    None
                }
                Ok(Variant::Attached(back_cover)) => {
                    info!("Unsupported TOH type connected");
                    back_cover.wait_disconnect().await?;
                    None
                }
                Err(error) => {
                    warn!("TOH power up failed: {error}");
                    sleep(Duration::from_secs(10)).await;
                    None
                }
            };
        // Wait a bit after powering up so the chip has a chance to be ready
        sleep(Duration::from_millis(100)).await;
        if let Some(mut back_cover) = detect {
            // TODO: Detection might fail because the TOH has not yet properly connected => we
            // should repeat until we get acknowledge or the TOH disconnects.
            match back_cover.detect().await {
                Ok(Some(info)) => {
                    let toh = Toh::new(info);
                    object_server.at(TOH_PATH, toh).await?;
                    // TODO: This should power down the TOH here if there is no need to keep it
                    // powered
                    back_cover.wait_disconnect_boxed().await?;
                    object_server.remove::<Toh, _>(TOH_PATH).await?;
                }
                Ok(None) => {
                    info!("TOH already disconnected");
                }
                Err(error) => {
                    warn!("Detecting TOH failed: {error:?}");
                    sleep(Duration::from_secs(1)).await;
                }
            }
        };
    }
}
