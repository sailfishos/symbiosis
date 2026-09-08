// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! Bus control.

use crate::i2cdev::I2cDev;
use std::fs::{self, read_dir, File};
use std::io::{self, Error, ErrorKind, Write};
use std::path::{Path, PathBuf};

// TOH I3C/I²C bus has address 11d00000
const BUS_PATH: &str = "/sys/devices/platform/soc/11d00000.i2c";

/// Small helper to aid writing to sysfs attributes.
#[derive(Clone, Debug)]
struct Attribute {
    path: PathBuf,
}

impl Attribute {
    fn write(&self, content: &[u8]) -> io::Result<()> {
        File::options()
            .create(false)
            .write(true)
            .open(&self.path)
            .and_then(|mut file| {
                file.write_all(content)?;
                file.sync_all()?;
                Ok(())
            })
    }
}

/// Finds the number of the TOH I²C bus.
///
/// This can be also used for determining the right i2c-dev character device.
pub(crate) fn find_toh_i2c_bus() -> io::Result<u8> {
    // Note that this would not handle multiple results properly but we don't expect more than one
    // matching directory in this path.
    read_dir(BUS_PATH)?
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
            "i2c directory was not found in i2c bus directory, is the driver loaded?",
        ))
}

/// Control devices available via I²C bus.
#[derive(Debug)]
pub struct I2cBus {
    bus: u8,
    new_device: Attribute,
    delete_device: Attribute,
}

impl I2cBus {
    /// Create new instance for TOH I²C bus.
    pub fn toh_bus() -> io::Result<Self> {
        let bus = find_toh_i2c_bus()?;
        log::debug!("Found TOH bus i2c-{bus}");
        let path = Path::new(BUS_PATH).join(format!("i2c-{bus}"));
        Ok(Self {
            bus,
            new_device: Attribute {
                path: path.join("new_device"),
            },
            delete_device: Attribute {
                path: path.join("delete_device"),
            },
        })
    }

    /// Create new [`I2cDev`] for this bus.
    pub fn i2c_dev(&self) -> io::Result<I2cDev> {
        I2cDev::for_bus(self.bus)
    }

    /// Adds I²C target device to the bus, optionally with a specified name.
    ///
    /// Use the returned [`I2cTarget`] to remove it afterwards.
    pub fn add_target(&mut self, address: u8, name: Option<&str>) -> io::Result<I2cTarget> {
        // TODO: It would make sense to add some kind of 'Attribute' wrapper for Path to always do
        // these steps when setting values.
        let name = name.unwrap_or("none");
        log::debug!(
            "Adding target 0x{address:02x} to i2c-{} for {name}",
            self.bus
        );

        // Check if the device already exists!
        let directory = self
            .new_device
            .path
            .with_file_name(format!("{}-{:04x}", self.bus, address));
        if directory.exists() {
            let current_name = fs::read_to_string(directory.join("name"))?;
            let current_name = current_name.trim_end();
            if current_name != name {
                // Wrong name, remove and add again
                log::debug!("Found {address:04x} with name '{current_name}', removing first");
                self.delete_device
                    .write(format!("0x{address:02x}\n").as_bytes())?;
            } else {
                // Already there
                log::debug!("Already have 0x{address:02x}");
                return Ok(I2cTarget::new(self, address));
            }
        }

        self.new_device
            .write(format!("{name} 0x{address:02x}\n").as_bytes())?;
        log::debug!("Added target 0x{address:02x}");
        Ok(I2cTarget::new(self, address))
    }

    /// Removes all devices from the bus.
    ///
    /// Returns [`Err`] if removing any target device fails but tries to remove all of them.
    pub(crate) fn remove_all_targets(&mut self) -> io::Result<()> {
        log::debug!("Removing all devices from i2c-{}", self.bus);
        // Find all directories that start with "N-", where N is bus number, get the addresses from
        // those names and try to remove _all_ of them.
        let filter = format!("{}-", self.bus);
        let failures = read_dir(BUS_PATH)?
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_type()
                    .map(|metadata| metadata.is_dir())
                    .unwrap_or(false)
            })
            .map(|entry| entry.path())
            .filter_map(|path| {
                let name = path.file_name()?.to_str()?;
                if name.starts_with(&filter) {
                    let (_, address) = name.split_at(filter.len());
                    u8::from_str_radix(address, 16).ok()
                } else {
                    None
                }
            })
            .map(|address| {
                log::debug!("Removing target 0x{address:02x} from i2c-{}", self.bus);
                self.delete_device
                    .write(format!("0x{address:02x}\n").as_bytes())
            })
            .filter_map(Result::err)
            .collect::<Vec<_>>();
        // Returning the first error if any.
        if let Some(error) = failures.into_iter().next() {
            Err(error)
        } else {
            Ok(())
        }
    }
}

/// Target device that has an address on the bus.
pub struct I2cTarget {
    delete_device: Attribute,
    address: u8,
}

impl I2cTarget {
    fn new(bus: &I2cBus, address: u8) -> Self {
        Self {
            delete_device: bus.delete_device.clone(),
            address,
        }
    }

    /// Remove the target device from the bus.
    pub fn remove(self) -> io::Result<()> {
        let I2cTarget {
            delete_device,
            address,
        } = self;
        log::debug!("Removing target 0x{address:02x}");
        delete_device.write(format!("0x{address:02x}\n").as_bytes())
    }
}
