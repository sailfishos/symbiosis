// Copyright (c) 2026 Jolla Mobile Ltd

//! I²C device.

use crate::back_cover::paths::I2C_PATH;
use libc::{self, ioctl};
use std::fs::File;
use std::io::{Error, Read, Result, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::path::Path;

// From Linux uapi
const I2C_SLAVE: libc::c_ulong = 0x0703;

/// Use i2c-dev driver to talk with I²C bus.
#[derive(Debug)]
pub struct I2cDev {
    file: File,
}

impl I2cDev {
    /// Create new instance for path.
    ///
    /// The path should be a device file like `"/dev/i2c-0"`.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        // TODO: This should check that the file is for the correct type of device.
        Ok(Self {
            file: File::options().read(true).write(true).open(path)?,
        })
    }

    /// Create new instance for TOH I²C bus.
    pub fn toh_dev() -> Result<Self> {
        // TODO: Get path properly to avoid accidentally writing something unintended
        I2cDev::new(I2C_PATH)
    }

    /// Create new instance from file descriptor.
    pub(crate) fn from_fd(fd: OwnedFd) -> Self {
        // TODO: Should this be able to fail somehow?
        Self { file: fd.into() }
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
