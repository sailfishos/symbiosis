// Copyright (c) 2026 Jolla Mobile Ltd

//! I²C device.

use crate::back_cover::paths::I2C_PATH;
use libc::{self, ioctl};
use std::fs::File;
use std::io::{Error, Read, Result, Write};
use std::os::{
    fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd},
    unix::fs::{FileTypeExt, MetadataExt},
};
use std::path::Path;

// From Linux uapi
const I2C_SLAVE: libc::c_ulong = 0x0703;

// From Linux admin guide
const I2C_DEV_MAJOR: libc::c_uint = 89;

/// Checks that the device is char device with major number 89 as those are what Linux uses for
/// i2c-dev devices.
fn is_i2c_dev_device(file: &File) -> Result<bool> {
    let metadata = file.metadata()?;
    let rdev = metadata.rdev();
    Ok(metadata.file_type().is_char_device() && libc::major(rdev) == I2C_DEV_MAJOR)
}

/// Use i2c-dev driver to talk with I²C bus.
#[derive(Debug)]
pub struct I2cDev {
    file: File,
}

impl I2cDev {
    /// Create new instance for path.
    ///
    /// The path must be a i2c-dev device file like `"/dev/i2c-0"`.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::options().read(true).write(true).open(path)?;
        if is_i2c_dev_device(&file)? {
            Ok(Self { file })
        } else {
            Err(Error::other("Path does not belong to i2c-dev device"))
        }
    }

    /// Create new instance for TOH I²C bus.
    pub fn toh_dev() -> Result<Self> {
        // TODO: Get path properly to avoid accidentally writing something unintended
        I2cDev::new(I2C_PATH)
    }

    /// Create new instance from file descriptor.
    pub(crate) fn from_fd(fd: OwnedFd) -> Result<Self> {
        let file: File = fd.into();
        if is_i2c_dev_device(&file)? {
            Ok(Self { file })
        } else {
            Err(Error::other("Fd does not belong to i2c-dev device"))
        }
    }

    /// Set I²C device address.
    pub fn set_target_address(&mut self, address: u32) -> Result<()> {
        let raw_fd = self.file.as_raw_fd();

        // SAFETY: This is the right ioctl number and arguments are suitable.
        let result = unsafe { ioctl(raw_fd, I2C_SLAVE, address) };
        if result < 0 {
            Err(Error::last_os_error())?
        }
        Ok(())
    }
}

impl Read for I2cDev {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.file.read(buf)
    }
}

impl Write for I2cDev {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> Result<()> {
        self.file.flush()
    }
}

impl AsFd for I2cDev {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }
}
