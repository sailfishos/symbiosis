// Copyright (c) 2026 Jolla Mobile Ltd

//! TOH memory chip writer

use libc::{self, ioctl};
use std::env;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Rem;
use std::os::fd::AsRawFd;
use std::thread::sleep;
use std::time::Duration;

use symbiosis::back_cover::paths::{ADC_PATH, I2C_PATH, INT_PATH, PWR_PATH};
use symbiosis::i2cdev::I2C_SLAVE;

/// Wait for INT pin to become 0
fn wait_for_int() -> Result<(), std::io::Error> {
    let mut int = File::open(INT_PATH)?;
    let mut lock = std::io::stdout().lock();
    write!(lock, "Waiting for INT pin to go low for up to a minute")?;
    lock.flush()?;
    for _ in 0..(60_000 / 500) {
        let value = io::read_to_string(&int)?;
        if value.trim_end() == "0" {
            writeln!(lock)?;
            return Ok(());
        }
        int.rewind()?;
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
    let adc = File::open(ADC_PATH)?;
    let value = io::read_to_string(&adc)?.trim_end().parse::<u32>()?;
    match value {
        // TODO: Change this to use the production values!
        600..=650 => {
            println!("TOH with memory chip (8 blocks of 256 bytes) detected");
            Ok(())
        }
        _ => Err(format!("Unexpected value on ADC: {}", value)
            .to_owned()
            .into()),
    }
}

/// Enable or disable power
fn set_power_with_silent(enable: bool, silent: bool) -> Result<(), std::io::Error> {
    let mut pwr = File::create(PWR_PATH)?;
    pwr.write_all(if enable { b"1" } else { b"0" })?;
    if !silent {
        println!("Power {}abled", if enable { "en" } else { "dis" });
    }
    sleep(Duration::from_millis(100));
    Ok(())
}

/// Enable or disable power
fn set_power(enable: bool) -> Result<(), std::io::Error> {
    set_power_with_silent(enable, false)
}

/// Set I²C device address
fn set_i2c_target_address(i2c: &mut File, address: u32) -> Result<(), std::io::Error> {
    let raw_fd = i2c.as_raw_fd();
    let result = unsafe { ioctl(raw_fd, I2C_SLAVE, address) };
    if result < 0 {
        Err(std::io::Error::last_os_error())?
    }
    Ok(())
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
fn write_chip(i2c: &mut File, file: &mut File) -> Result<(), Box<dyn std::error::Error>> {
    // Let's check how many bytes we have to read
    let size = get_file_size(file)?;
    // TODO: Support other types of memory chips
    if size > 256 * 8 {
        // The chip has 8 blocks of 256 bytes
        Err(format!(
            "Too big input file: {} bytes (must be < {} bytes)",
            size,
            256 * 8
        )
        .to_owned())?
    }
    let size = size as u32;

    // Read from file, write to the chip
    let mut lock = std::io::stdout().lock();
    write!(lock, "Writing {} bytes", size)?;
    lock.flush()?;

    // TODO: Support other page sizes
    let mut buf = [0; 17]; // 1 byte for address and 16 byte pages
    let mut written: usize = 0;

    for address in 0x50..0x50 + size.div_ceil(256) {
        set_i2c_target_address(i2c, address)?;

        let start = written - written.rem(256);
        while written < start + 256 {
            let data_address = written - start;
            let length = (256 - data_address).min(16);
            let length = file.read(&mut buf[1..length + 1])?;
            if length == 0 {
                break;
            }
            buf[0] = data_address as u8;
            i2c.write_all(&buf[..length + 1])?;
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
fn verify_chip(i2c: &mut File, file: &mut File) -> Result<(), Box<dyn std::error::Error>> {
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

        set_i2c_target_address(i2c, address)?;

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
            Err("Verification failed".to_owned())?
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
        Err("Not enough bytes read from memory chip".to_owned().into())
    }
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os();
    args.next().unwrap(); // Skip program name
    let file = args.next().ok_or("File name required".to_owned())?;
    if args.count() != 0 {
        Err("Too many arguments".to_owned())?
    }
    // TODO: Add page size argument for writing

    // TODO: Modprobe i2c-dev if it is not there yet

    let mut file = File::open(file)?;

    wait_for_int()?;

    test_adc_pin()?;

    set_power(true)?;

    // Open the file for reading and writing
    let mut i2c = File::options().read(true).write(true).open(I2C_PATH)?;

    write_chip(&mut i2c, &mut file)
        .and_then(|()| verify_chip(&mut i2c, &mut file))
        // TODO: Rust 1.76.0 would let us use Result::inspect_err() here
        .map_err(|err| {
            // If writing or verifying failed, try to turn off power
            let _ = set_power_with_silent(false, true);
            err
        })?;

    set_power(false)?;

    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("This code works only on Linux!");
}
