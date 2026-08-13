// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! TOH blinker for TOHs with AW2023 controller.
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

#[derive(FromArgs)]
#[argh(description = "Control TOH LEDs")]
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
            if let Some(ExtraValue::Text(controller)) = info.extra.get("led-controller") {
                if controller == "aw2023" {
                    println!(
                        "Found TOH {:04x}:{:04x} with aw2023 controller",
                        info.vendor_id, info.product_id
                    );
                } else {
                    return Err("Unknown controller: {controller}".into());
                }
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
