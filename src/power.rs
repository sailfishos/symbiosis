// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! Power output pin handling.
use crate::attr::{access, Attribute};
use std::io::{self, ErrorKind};
use std::marker::Send;
use std::path::Path;

pub use crate::attr::{Read, ReadWrite};

// TODO: Drop the old path once the new driver is in a release.
const PWR_PATH: &str = "/sys/devices/platform/yft_pogo_pin/power_request";
const OLD_PWR_PATH: &str = "/sys/class/yft_pogo_pin/yft_pogo_pin_5v_out_state";

/// Power output pin state handling.
pub struct Power<A: access::Access + Send> {
    attr: Attribute<A>,
}

impl Power<Read> {
    /// Create new power output pin state reader.
    ///
    /// This uses the device file directly.
    pub fn read_only() -> std::io::Result<Self> {
        let path = Path::new(PWR_PATH);
        Ok(Self {
            attr: Attribute::readable(if path.exists() {
                path
            } else {
                Path::new(OLD_PWR_PATH)
            })?,
        })
    }
}

impl Power<ReadWrite> {
    /// Create new power output pin state handler.
    ///
    /// This uses the device file directly.
    pub fn new() -> std::io::Result<Self> {
        let path = Path::new(PWR_PATH);
        Ok(Self {
            attr: Attribute::read_and_writable(if path.exists() {
                path
            } else {
                Path::new(OLD_PWR_PATH)
            })?,
        })
    }
}

impl<A: access::Access + access::Write + Send> Power<A> {
    /// Set power output.
    pub fn set_power(&mut self, enabled: bool) -> std::io::Result<()> {
        self.attr.write(if enabled { b"1" } else { b"0" })?;
        Ok(())
    }
}

impl<A: access::Access + access::Read + Send> Power<A> {
    /// Check if power output is enabled.
    pub fn is_powered(&mut self) -> std::io::Result<bool> {
        match self
            .attr
            .read_to_string()?
            .trim_end()
            .parse::<u8>()
            .map_err(|err| io::Error::new(ErrorKind::InvalidData, err.to_string()))?
        {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(io::Error::new(
                ErrorKind::InvalidData,
                "Invalid power out state",
            )),
        }
    }
}
