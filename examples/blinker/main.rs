// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! Inari Blue TOH blinker.
//!
//! This is just an example of how the chip in the TOH can be used via the tohd1 D-Bus interface and
//! i2c-dev driver. Actual functionality should be implemented elsewhere.

mod controller;

use crate::controller::LedController;
use argh::FromArgs;
use std::time::Duration;
use symbiosis::toh::*;
use tokio::time::sleep;
use zbus::fdo::Error as DBusError;

const INARI_BLUE_TOH_VENDOR_ID: u16 = 1;
const INARI_BLUE_TOH_PRODUCT_ID: u16 = 4;

#[derive(FromArgs)]
#[argh(description = "Control Inari Blue LEDs")]
struct Arguments {
    #[argh(subcommand)]
    command: Command,
}

// TODO: Add more subcommands to do other nice things
#[derive(FromArgs)]
#[argh(subcommand)]
enum Command {
    Off(Off),
    Breathing(Breathing),
}

#[derive(FromArgs)]
#[argh(subcommand, name = "off", description = "Turn off the LEDs")]
struct Off {}

#[derive(FromArgs)]
#[argh(subcommand, name = "breathing", description = "Play breathing effect")]
struct Breathing {}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Arguments = argh::from_env();

    let mut toh = Toh::new().await?;
    let needs_power_on = !(&toh).is_powered().await?;
    // TODO: Avoid errors when reacting to disconnect of a TOH
    match (&toh).detect().await {
        Ok(Some(info)) => {
            if info.vendor_id == INARI_BLUE_TOH_VENDOR_ID
                && info.product_id == INARI_BLUE_TOH_PRODUCT_ID
            {
                println!("Found Inari Blue TOH");
                // Leave power on if some effect is running.
                let leave_powered = !matches!(args.command, Command::Off(_));
                // The library makes sure we use the borrowing interface correctly here.
                if let Err(error) = toh
                    .access_i2c_dev_with_power(leave_powered, move |dev| {
                        Box::pin(async move {
                            let mut ctrl = LedController::new(dev);
                            if needs_power_on {
                                sleep(Duration::from_millis(500)).await;
                            }
                            match args.command {
                                Command::Breathing(_) => {
                                    println!("Turning on breathing effect");
                                    ctrl.start_breathing().await
                                }
                                Command::Off(_) => {
                                    println!("Turning LED off");
                                    ctrl.turn_led_off()
                                }
                            }
                        })
                    })
                    .await?
                {
                    Err(format!("Failed to blink LED: {error}").into())
                } else {
                    Ok(())
                }
            } else {
                Err("This is not the TOH we are looking for".into())
            }
        }
        Ok(None) => Err("TOH is not present or is of unsupported type".into()),
        Err(FetchingInfoError::DBus(DBusError::ServiceUnknown(..))) => {
            Err("TOH daemon is not running".into())
        }
        Err(FetchingInfoError::DBus(error)) => {
            // Some other DBus related error
            Err(format!("D-Bus error: {:?}", error).into())
        }
        Err(FetchingInfoError::Conversion(error)) => {
            // Would not expect this kind of errors but who knows
            Err(format!("Conversion error: {error}").into())
        }
    }
}
