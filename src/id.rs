// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! ID pin handling, ADC and all that stuff.

use crate::back_cover::paths::ADC_PATH;
use derive_more::Into;
use std::fs::File;
use std::io::{self, ErrorKind, Seek};
use std::ops::Sub;

/// Identified TOH types according to read ADC value.
pub enum TohId {
    /// Nominally 10kΩ resistor.
    R10k,
    /// Nominally 15kΩ resistor.
    R15k,
    /// Unknown value resistor.
    Unknown,
    /// Resistor not present or value too high to detect.
    NotPresent,
}

/// Value read from ID pin ADC.
///
/// Convertible to `u16` for the inner value.
#[derive(Copy, Clone, Debug, Default, Ord, PartialOrd, PartialEq, Eq, Into)]
pub struct AdcValue(u16);

impl AdcValue {
    /// Whether TOH is present according to the value.
    ///
    /// Note that implementations should also observe interrupt pin.
    pub fn is_toh_present(&self) -> bool {
        // TODO: Check the limits, we aim for 0 - 1.7 volts.
        (0..1750).contains(&self.0)
    }

    /// Identify the resistor in TOH.
    pub fn identify(&self) -> TohId {
        match self.0 {
            800..=999 => TohId::R10k,
            1000..=1199 => TohId::R15k,
            1750.. => TohId::NotPresent,
            _ => TohId::Unknown,
        }
    }
}

impl From<AdcValue> for f64 {
    fn from(value: AdcValue) -> Self {
        value.0.into()
    }
}

impl Sub for AdcValue {
    type Output = i32;

    fn sub(self, rhs: Self) -> Self::Output {
        i32::from(self.0) - i32::from(rhs.0)
    }
}

/// ID pin state.
pub struct Id {
    file: File,
}

impl Id {
    /// Create new ID pin state handler.
    ///
    /// This uses the device file directly.
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            file: File::open(ADC_PATH)?,
        })
    }

    /// Read the current ID pin state with ADC.
    pub fn read(&mut self) -> std::io::Result<AdcValue> {
        self.file.rewind()?;
        Ok(AdcValue(
            io::read_to_string(&self.file)?
                .trim_end()
                .parse::<u16>()
                .map_err(|err| io::Error::new(ErrorKind::InvalidData, err.to_string()))?,
        ))
    }
}
