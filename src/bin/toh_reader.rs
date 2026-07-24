// Copyright (c) 2026 Jolla Mobile Ltd

//! TOH memory chip reader.

use argh::FromArgs;
use std::fs::File;
use std::io::Write;
use std::time::Duration;
use tokio::{select, time::sleep};

use symbiosis::back_cover::{BackCover, PowerDown, Variant};

/// TOH memory chip reader.
///
/// Supports chips with 256 byte blocks.
#[derive(FromArgs)]
#[argh(help_triggers("-h", "--help"))]
struct Arguments {
    /// output file.
    #[argh(positional)]
    output_file: String,
    /// force overwrite
    #[argh(switch, short = 'f', long = "force")]
    overwrite: bool,
}

fn read_content(back_cover: Variant) -> std::io::Result<Option<Vec<u8>>> {
    match back_cover {
        Variant::With256BBlocks(mut back_cover) => {
            println!("TOH with memory chip (up to 16 blocks of 256 bytes) detected");
            let content = back_cover.read_chip()?;
            back_cover.power_down()?;
            Ok(Some(content))
        }
        Variant::With64kBBlocks(back_cover) => {
            println!("Unsupported TOH");
            // TODO: Implement
            back_cover.power_down()?;
            Ok(None)
        }
        Variant::Attached(back_cover) => {
            println!("Unsupported TOH");
            back_cover.power_down()?;
            Ok(None)
        }
    }
}

#[cfg(target_os = "linux")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Arguments = argh::from_env();
    let mut file = File::options()
        .create(true)
        .create_new(!args.overwrite)
        .write(true)
        .truncate(true)
        .open(args.output_file)?;

    println!("Waiting for TOH to be connected for up to a minute");
    let back_cover = BackCover::new()?;
    let back_cover = select! {
        result = back_cover.wait_connect() => {
            Ok(result?)
        }
        _ = sleep(Duration::from_secs(60)) => {
            Err("No TOH detected")
        }
    }?;
    match back_cover.power_up().await {
        Ok(variant) => {
            if let Some(content) = read_content(variant)? {
                println!("Read {} bytes from the chip", content.len());
                file.write_all(content.as_slice())?;
            } else {
                return Err("TOH could not be read".into());
            }
        }
        Err(error) => {
            return Err(format!("TOH power up failed: {error}").into());
        }
    };

    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("This code works only on Linux!");
}
