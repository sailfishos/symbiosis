// Copyright (c) 2026 Jolla Mobile Ltd

//! TOH memory chip reader.

use argh::FromArgs;
use libc::{self, ioctl};
use std::fs::File;
use std::io::{self, Read, Seek, Write};
use std::os::fd::AsRawFd;
use std::thread::sleep;
use std::time::Duration;

use symbiosis::back_cover::paths::{ADC_PATH, I2C_PATH, INT_PATH, PWR_PATH};
use symbiosis::i2cdev::I2C_SLAVE;

/// TOH memory chip reader.
#[derive(FromArgs)]
struct Arguments {
    /// output file.
    #[argh(positional)]
    output_file: String,
}

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

/// Get file size
/// Use I²C to read the chip into a vector
fn read_chip(i2c: &mut File) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut result = Vec::new();
    let mut lock = std::io::stdout().lock();
    write!(lock, "Reading the chip for up to {} bytes", 256 * 8)?;
    lock.flush()?;
    let mut buf = [0; 256];
    let mut read = 0_u64;
    for address in 0x50..0x58 {
        set_i2c_target_address(i2c, address)?;

        // Set data address to zero
        i2c.write_all(&[0])?;

        i2c.read_exact(&mut buf)?;
        result.extend(buf);

        read += 256;
        write!(lock, ".")?;
        lock.flush()?;
    }
    writeln!(lock)?;
    writeln!(lock, "{} bytes read", read)?;
    Ok(result)
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Arguments = argh::from_env();
    // TODO: Add page size argument for writing

    // TODO: Modprobe i2c-dev if it is not there yet

    let mut file = File::options()
        .create(true)
        .write(true)
        .truncate(true)
        .open(args.output_file)?;

    wait_for_int()?;

    test_adc_pin()?;

    set_power(true)?;

    // Open the file for reading and writing
    let mut i2c = File::options().read(true).write(true).open(I2C_PATH)?;

    let result = read_chip(&mut i2c)?;
    file.write_all(result.as_ref())?;

    set_power(false)?;

    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("This code works only on Linux!");
}
