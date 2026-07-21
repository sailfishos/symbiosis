// Copyright (c) 2026 Jolla Mobile Ltd

//! Create binary for TOH from input.

use argh::FromArgs;
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::Path;
use symbiosis::toh::Info;

/// Create binary for TOH from input.
#[derive(FromArgs)]
struct Arguments {
    /// input file.
    #[argh(positional)]
    input_file: String,
    /// output file.
    #[argh(positional)]
    output_file: String,
}

fn read_yaml(path: &Path) -> Result<Info, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let info = yaml_serde::from_reader(&mut reader)?;
    Ok(info)
}

fn print_info(info: &Info) {
    let Info {
        vendor_id,
        product_id,
        serial_number,
        vendor_name,
        product_name,
        vendor_website,
        product_website,
        leave_power_on,
        power_input_toh,
        ..
    } = info;
    if let Some(name) = vendor_name {
        println!("Vendor: {} ({:#06x})", name, vendor_id);
    } else {
        println!("Vendor: {:#06x}", vendor_id);
    }
    if let Some(name) = product_name {
        println!("Product: {} ({:#06x})", name, product_id);
    } else {
        println!("Product: {:#06x}", product_id);
    }
    if let Some(value) = serial_number {
        println!("Serial number: {} ", value);
    }
    if let Some(value) = vendor_website {
        println!("Vendor website: {} ", value);
    }
    if let Some(value) = product_website {
        println!("Product website: {} ", value);
    }
    if let Some(value) = leave_power_on {
        println!("Power out enable: {} ", value);
    }
    if let Some(value) = power_input_toh {
        println!("Power input TOH: {} ", value);
    }
}

fn write_binary(path: &Path, info: Info) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = File::create(path)?;
    let content = info.into_bytes()?;
    // TODO: This could parse the result in buff and check that everything is ok
    file.write_all(content.as_slice())?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Arguments = argh::from_env();
    let info = read_yaml(Path::new(&args.input_file))?;
    print_info(&info);
    write_binary(Path::new(&args.output_file), info)?;
    Ok(())
}
