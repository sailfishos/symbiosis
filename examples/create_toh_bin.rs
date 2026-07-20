// Copyright (c) 2026 Jolla Mobile Ltd

//! Create binary for TOH from input

use ciborium::Value;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::io::{BufReader, Cursor, Write};
use std::path::Path;

#[derive(Deserialize)]
struct Info {
    vendor_id: u16,
    product_id: u16,
    serial_number: Option<String>,
    vendor_name: Option<String>,
    product_name: Option<String>,
    vendor_website: Option<String>,
    product_website: Option<String>,
    power_out: Option<bool>,
    power_in: Option<bool>,
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
        power_out,
        power_in,
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
    if let Some(value) = power_out {
        println!("Power out enable: {} ", value);
    }
    if let Some(value) = power_in {
        println!("Power input TOH: {} ", value);
    }
}

fn write_binary(path: &Path, info: Info) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = File::create(path)?;

    let mut buff = Cursor::new(Vec::with_capacity(16));
    let mut payload = BTreeMap::<String, Value>::new();
    let Info {
        vendor_id,
        product_id,
        serial_number,
        vendor_name,
        product_name,
        vendor_website,
        product_website,
        power_out,
        power_in,
    } = info;
    if let Some(value) = serial_number {
        payload.insert("SN".to_string(), Value::Text(value));
    }
    if let Some(value) = vendor_name {
        payload.insert("VN".to_string(), Value::Text(value));
    }
    if let Some(value) = product_name {
        payload.insert("PN".to_string(), Value::Text(value));
    }
    if let Some(value) = vendor_website {
        payload.insert("VS".to_string(), Value::Text(value));
    }
    if let Some(value) = product_website {
        payload.insert("PS".to_string(), Value::Text(value));
    }
    if let Some(value) = power_out {
        payload.insert("PO".to_string(), Value::Bool(value));
    }
    if let Some(value) = power_in {
        payload.insert("PI".to_string(), Value::Bool(value));
    }
    buff.write(&[0x4A, 0x54, 0x4F, 0x48])?;
    buff.write(&0_u32.to_be_bytes())?; // Placeholder for checksum
    buff.write(&vendor_id.to_be_bytes())?;
    buff.write(&product_id.to_be_bytes())?;
    buff.write(&0_u16.to_be_bytes())?; // Padding
    buff.write(&0_u16.to_be_bytes())?; // Zero for size
    assert!(buff.get_ref().len() == 16);

    // If we have a payload write that too and update size
    if !payload.is_empty() {
        payload.insert("SC".to_string(), Value::Integer(0.into()));

        ciborium::into_writer(&payload, &mut buff)?;

        // Update size field
        let size = (buff.get_ref().len() - 16) as u16;
        buff.set_position(0x0e);
        buff.write(&size.to_be_bytes())?;
    }

    // Update checksum
    let data = buff.get_ref();
    let checksum = crc32fast::hash(&data[0x08..]);
    buff.set_position(0x04);
    buff.write(&checksum.to_be_bytes())?;

    // TODO: This could parse the result in buff and check that everything is ok

    // Write to file
    file.write(buff.get_ref().as_slice())?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os();
    if args.len() != 3 {
        Err("Usage: create_toh_bin [infile] [outfile]")?
    };
    args.next().unwrap(); // Skip program name
    let input = args.next().unwrap();
    let output = args.next().unwrap();

    let info = read_yaml(Path::new(&input))?;
    print_info(&info);
    write_binary(Path::new(&output), info)?;
    Ok(())
}
