// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! Prints a line for each interrupt received.
//!
//! Closes when TOH is disconnected.

use argh::FromArgs;
use symbiosis::toh::*;
use zbus::fdo::Error as DBusError;

#[derive(FromArgs)]
#[argh(
    help_triggers("-h", "--help"),
    description = "Wait for interrupts.

This waits for interrupts to happen on interrupt pin and prints a line for every interrupt received.
"
)]
struct Arguments {}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _args: Arguments = argh::from_env();
    let mut toh = Toh::new().await?;
    use SubscribeInterruptsError::*;
    match toh.subscribe_interrupts().await {
        Ok(mut receiver) => loop {
            match receiver.wait_interrupt().await {
                Ok(Interrupt) => {
                    println!("Interrupt");
                }
                Err(WaitInterruptError::Disconnected) => {
                    return Ok(());
                }
                Err(WaitInterruptError::Io(error)) => {
                    return Err(format!("IO error: {error}").into());
                }
            }
        },
        Err(DBus(DBusError::UnknownObject(..))) => {
            Err("TOH is not present or is of unsupported type".into())
        }
        Err(DBus(DBusError::ServiceUnknown(..))) => Err("TOH daemon is not running".into()),
        Err(Subscribe(SubscribeError::AccessDenied)) => {
            Err("Access denied, please configure TOH daemon".into())
        }
        Err(Subscribe(error)) => Err(format!("Subscribe error: {error:?}").into()),
        Err(DBus(error)) => Err(format!("D-Bus error: {error:?}").into()),
        Err(Io(error)) => Err(format!("IO error: {error}").into()),
    }
}
