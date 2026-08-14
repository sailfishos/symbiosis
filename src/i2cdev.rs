// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! I²C device.

use libc::{self, ioctl};
use std::fs::{read_dir, File};
use std::io::{Error, ErrorKind, Read, Result, Write};
use std::os::{
    fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd},
    unix::fs::{FileTypeExt, MetadataExt},
};
use std::path::Path;

// From Linux uapi
const I2C_SLAVE: libc::c_ulong = 0x0703;

// From Linux admin guide
const I2C_DEV_MAJOR: libc::c_uint = 89;

// TOH I²C bus has address 11d00000
const I2C_BUS_PATH: &str = "/sys/devices/platform/soc/11d00000.i2c";

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
    ///
    /// Note that TOH bus might not be actually `"/dev/i2c-0"`, thus it is much better to use
    /// [`toh_dev`](Self::toh_dev) instead of this when trying to access TOH I²C bus.
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
        // Note that this would not handle multiple results properly
        let number = read_dir(I2C_BUS_PATH)?
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_type()
                    .map(|metadata| metadata.is_dir())
                    .unwrap_or(false)
            })
            .map(|entry| entry.file_name())
            .filter_map(|name| {
                let name = name.to_str()?;
                if name.starts_with("i2c-") {
                    let (_, number) = name.split_at(4);
                    number.parse::<u8>().ok()
                } else {
                    None
                }
            })
            .next()
            .ok_or(Error::new(
                ErrorKind::NotFound,
                "i2c-dev directory was not found in i2c bus directory, is the driver loaded?",
            ))?;
        let path = format!("/dev/i2c-{number}");
        log::debug!("Found TOH i2c-dev device '{path}'");
        I2cDev::new(path)
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
