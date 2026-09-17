// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! A simple I²C read test to read the memory chip.
//!
//! This reads the single block multiple times over and over again to estimate how fast it can be
//! read.
//!
//! Note that this measures I²C bus speed, memory chip read speed, protocol overhead and kernel
//! overhead all at the same time and cannot distinguish between them. In any case, it should give a
//! realistic idea what to expect when communicating over I²C.
//!
//! Also this does not adjust the bus speed before the test so it will use whatever is configured by
//! the kernel at the time of the test.

use argh::FromArgs;
use std::io::{Error, Read};
use std::time::Instant;
use symbiosis::toh::*;
use zbus::fdo::Error as DBusError;

#[derive(FromArgs)]
#[argh(
    help_triggers("-h", "--help"),
    description = "TOH I²C memory chip read speed test.

Supports at24c{{01,02,04,08,16}} compatible chips like the ones in TOHs with 10 kohm resistor.
Enable the test by symlinking the example configuration to your TOHs directory. E.g.

    ln -s ../../../examples/i2c-speed-test.yaml /usr/share/tohd-1/tohs/0001/0001/

And reconnect TOH.
"
)]
struct Arguments {
    /// force test for unknown TOHs
    #[argh(switch, short = 'f', long = "force")]
    force: bool,
    /// test length in bytes
    #[argh(option, short = 'l', long = "length", default = "2_usize.pow(14)")]
    length: usize,
}

const SUPPORTED_CHIPS: [&str; 5] = ["at24c01", "at24c02", "at24c04", "at24c08", "at24c16"];

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Arguments = argh::from_env();

    let mut toh = Toh::new().await?;
    match (&toh).detect().await {
        Ok(Some(info)) => {
            if let Some(ExtraValue::Text(chip)) = info.extra.get("memory-chip") {
                if SUPPORTED_CHIPS.contains(&chip.as_str()) {
                    println!(
                        "Found TOH {:04x}:{:04x} with {} compatible eeprom",
                        info.vendor_id, info.product_id, chip
                    );
                } else {
                    return Err(format!(
                        "Incompatible eeprom {} for TOH {:04x}:{:04x}. Cannot continue",
                        chip, info.vendor_id, info.product_id
                    )
                    .into());
                }
            } else if args.force {
                println!(
                    "Found TOH {:04x}:{:04x} with unknown eeprom",
                    info.vendor_id, info.product_id
                );
                println!(
                    "Memory chip is not specified but '--force' was given, trying to read anyway"
                );
            } else {
                return Err(format!(
                    "Unknown TOH {:04x}:{:04x}. Configuration may be missing",
                    info.vendor_id, info.product_id
                )
                .into());
            }
            let address = if let Some(ExtraValue::U64(address)) = info.extra.get("address") {
                (*address).try_into().ok()
            } else {
                None
            }
            .unwrap_or(0x50);
            if let Err(error) = toh
                .access_i2c_dev(move |dev| {
                    Box::pin(async move {
                        dev.set_target_address(address)?;
                        let mut output = vec![0; args.length];
                        let output = output.as_mut_slice();
                        assert_eq!(output.len(), args.length);
                        let before = Instant::now();
                        dev.read_exact(output)?;
                        let elapsed = before.elapsed();
                        println!(
                            "Reading {} B took {:.3} ms -> {:.3} kB/s",
                            output.len(),
                            elapsed.as_micros() as f64 / 1000.0,
                            output.len() as f64 / (elapsed.as_micros() as f64 / 1_000.0)
                        );
                        // Start + address + output bytes + ack bits + stop.
                        let cycles = 1 + 9 + output.len() * 9 + 1;
                        println!(
                            "Estimated {} bus clock cycles -> estimated speed {:.3} kHz",
                            cycles,
                            cycles as f64 / (elapsed.as_micros() as f64 / 1_000.0)
                        );
                        println!("Note that this speed estimation includes kernel overhead and does not represent real bus speed");
                        Ok::<_, Error>(())
                    })
                })
                .await?
            {
                Err(format!("Failed to measure memory chip speed: {error}").into())
            } else {
                Ok(())
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
