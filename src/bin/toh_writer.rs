// Copyright (c) 2026 Jolla Mobile Ltd

//! TOH memory chip writer.

use argh::FromArgs;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::ops::Rem;
use std::thread::sleep;
use std::time::Duration;

use symbiosis::back_cover::paths::I2C_PATH;
use symbiosis::i2cdev::I2CDev;
use symbiosis::id::{Id, TohId};
use symbiosis::interrupt::{IntState, Interrupt};
use symbiosis::power::Power;

/// TOH memory chip writer.
///
/// Uses i2c-dev to write chips. Only supports chips with 256 bytes * 8 blocks.
#[derive(FromArgs)]
#[argh(help_triggers("-h", "--help"))]
struct Arguments {
    /// input file.
    #[argh(positional)]
    input_file: String,
    /// page size.
    #[argh(option, short = 'p', default = "16")]
    page_size: u8,
}

/// Wait for INT pin to become 0
fn wait_for_int() -> Result<(), std::io::Error> {
    let mut int = Interrupt::new()?;
    let mut lock = std::io::stdout().lock();
    write!(lock, "Waiting for INT pin to go low for up to a minute")?;
    lock.flush()?;
    for _ in 0..(60_000 / 500) {
        if int.state()? == IntState::Low {
            writeln!(lock)?;
            return Ok(());
        }
        write!(lock, ".")?;
        lock.flush()?;
        sleep(Duration::from_millis(500));
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "TOH was not connected",
    ))
}

/// Test ADC for the right type of chip
fn test_adc_pin() -> Result<(), Box<dyn std::error::Error>> {
    let mut id = Id::new()?;
    let value = id.read()?;
    match value.identify() {
        TohId::R10k => {
            println!("TOH with memory chip (8 blocks of 256 bytes) detected");
            Ok(())
        }
        TohId::R15k | TohId::Unknown => Err("Unsupported TOH".into()),
        TohId::NotPresent => Err("Missing TOH".into()),
    }
}

/// Runs the function with power on.
fn with_power<F: FnOnce() -> Result<(), Box<dyn std::error::Error>>>(
    f: F,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut power = Power::new()?;
    power.set_power(true)?;
    println!("Power enabled");
    sleep(Duration::from_millis(100));

    let result = f();

    if result.is_ok() {
        power.set_power(false)?;
        println!("Power disabled");
    } else {
        let _ = power.set_power(false);
    }

    result
}

/// Get file size.
///
/// Rewinds the offset to beginning of file.
fn get_file_size(file: &mut File) -> Result<u64, std::io::Error> {
    let size = file.seek(SeekFrom::End(0))?;
    file.rewind()?;
    Ok(size)
}

/// Use I²C to write the chip
fn write_chip(
    i2c: &mut I2CDev,
    file: &mut File,
    page_size: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    // Let's check how many bytes we have to read
    let size = get_file_size(file)?;
    // TODO: Support other types of memory chips
    if size > 256 * 8 {
        // The chip has 8 blocks of 256 bytes
        Err(format!(
            "Too big input file: {} bytes (must be < {} bytes)",
            size,
            256 * 8
        ))?
    }
    let size = size as u32;

    // Read from file, write to the chip
    let mut lock = std::io::stdout().lock();
    write!(lock, "Writing {} bytes", size)?;
    lock.flush()?;

    // Buffer to contain page and 1 byte for address
    let mut buf = vec![0u8; page_size + 1];
    let mut written: usize = 0;

    for address in 0x50..0x50 + size.div_ceil(256) {
        i2c.set_target_address(address)?;

        let start = written - written.rem(256);
        while written < start + 256 {
            let data_address = written - start;
            let length = (256 - data_address).min(page_size);
            let length = file.read(&mut buf[1..length + 1])?;
            if length == 0 {
                break;
            }
            buf[0] = data_address as u8;
            i2c.write_all(&buf[..length + 1])?;
            // TODO: This should wait for ack instead
            sleep(Duration::from_millis(length as u64 * 7)); // Typically one byte takes 7 ms
            written += length;
            write!(lock, ".")?;
            lock.flush()?;
        }
    }
    writeln!(lock)?;
    writeln!(lock, "{} bytes written", written)?;
    Ok(())
}

/// Use I²C to verify the chip
fn verify_chip(i2c: &mut I2CDev, file: &mut File) -> Result<(), Box<dyn std::error::Error>> {
    // Let's check how many bytes we have to verify
    let size = get_file_size(file)?;

    // Read from file and from the chip
    let mut lock = std::io::stdout().lock();
    write!(lock, "Verifying {} bytes", size)?;
    lock.flush()?;
    let mut buf1 = [0; 256];
    let mut buf2 = [0; 256];
    let mut verified = 0_u64;
    for address in 0x50.. {
        assert!(
            verified <= size,
            "Verified size {} is larger than size {}!",
            verified,
            size
        );

        if verified == size {
            break;
        }

        i2c.set_target_address(address)?;

        // Set data address to zero
        i2c.write_all(&[0])?;

        let length = if size - verified < 256 {
            // Partial page read
            file.read(&mut buf1)?
        } else {
            // Full page read
            file.read_exact(&mut buf1)?;
            buf1.len()
        };

        if length == 0 {
            // Odd but ok
            break;
        }

        i2c.read_exact(&mut buf2[..length])?;
        if buf1[..length] != buf2[..length] {
            // Fill partial buffers with zeros just for debug logging
            buf1[length..].fill(0);
            buf2[length..].fill(0);
            // And then print
            writeln!(lock)?;
            writeln!(
                lock,
                "Verification failed at {}..{}!",
                verified,
                verified + length as u64,
            )?;
            writeln!(lock, "Expected ({} bytes): {:?}", length, buf1)?;
            writeln!(lock, "Got ({} bytes): {:?}", length, buf2)?;
            Err("Verification failed")?
        }

        verified += length as u64;
        write!(lock, ".")?;
        lock.flush()?;
    }
    writeln!(lock)?;
    if size == verified {
        writeln!(lock, "{} bytes verified", verified)?;
        Ok(())
    } else {
        writeln!(lock, "Only {} bytes verified from memory chip", verified)?;
        Err("Not enough bytes read from memory chip".into())
    }
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Arguments = argh::from_env();
    // TODO: Modprobe i2c-dev if it is not there yet
    // TODO: Or use i2c-dev handed over by tohd interface if there is one
    let mut file = File::open(args.input_file)?;
    wait_for_int()?;
    test_adc_pin()?;
    with_power(|| {
        let mut i2c = I2CDev::new(I2C_PATH)?;
        write_chip(&mut i2c, &mut file, args.page_size.into())
            .and_then(|()| verify_chip(&mut i2c, &mut file))
    })
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("This code works only on Linux!");
}
