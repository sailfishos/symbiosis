// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! Power handling.
//!
//! The kernel exposes 5V power output and bus power as voltage regulators. TOH driver allows to
//! request these to be powered on but other kernel drivers may also turn them on.

use crate::attr::{access, Attribute};
use crate::back_cover::paths::PWR_PATH;
use std::fs::{self, read_dir};
use std::io::{self, Error, ErrorKind};
use std::marker::Send;
use std::path::Path;
use thiserror::Error;

pub use crate::attr::{Read, ReadWrite};

/// Voltage regulator.
struct Regulator {
    attr: Attribute<Read>,
}

impl Regulator {
    /// Create new voltage regulator for path.
    fn new<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        Ok(Self {
            attr: Attribute::readable(path.as_ref().join("state"))?,
        })
    }

    /// Check if voltage regulator is enabled.
    fn is_enabled(&self) -> io::Result<bool> {
        let state = self.attr.read_to_string()?;
        let state = state.trim_end();
        match state {
            "enabled" => Ok(true),
            "disabled" => Ok(false),
            _ => Err(io::Error::new(
                ErrorKind::InvalidData,
                "Invalid regulator state",
            )),
        }
    }
}

/// TOH voltage regulators.
struct Regulators {
    /// 5V out voltage regulator.
    power: Regulator,
    /// Bus voltage regulator.
    bus: Regulator,
}

impl Regulators {
    /// Finds voltage regulators and creates them.
    fn new() -> io::Result<Self> {
        let mut power = None;
        let mut bus = None;
        let regulators_dir = Path::new(PWR_PATH)
            .parent()
            .expect("The path has parent")
            .join("regulator");
        for path in read_dir(regulators_dir)?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                if path
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .map(|name| name.starts_with("regulator."))
                    .unwrap_or(false)
                {
                    Some(path)
                } else {
                    None
                }
            })
        {
            let name = fs::read_to_string(path.join("name"))?;
            let name = name.trim_end();
            if name == "toh-5v" {
                power = Some(path);
            } else if name == "toh-bus" {
                bus = Some(path);
            }
        }
        if let Some(power) = power {
            if let Some(bus) = bus {
                Ok(Self {
                    power: Regulator::new(power)?,
                    bus: Regulator::new(bus)?,
                })
            } else {
                Err(Error::other("Bus voltage regulator device not found"))
            }
        } else {
            Err(Error::other("5V out voltage regulator device not found"))
        }
    }
}

/// Power output pin state handling.
pub struct Power<A: access::Access + Send> {
    power_request: Attribute<A>,
    reg: Regulators,
}

/// Power state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    /// Power is off.
    Off,
    /// 5V out is enabled but bus is not powered.
    Out,
    /// Bus is powered and 5V out is enabled.
    Bus,
}

impl State {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "0" => Some(State::Off),
            "1" => Some(State::Out),
            "2" => Some(State::Bus),
            _ => None,
        }
    }

    fn to_bytes(self) -> &'static [u8] {
        match self {
            State::Off => b"0",
            State::Out => b"1",
            State::Bus => b"2",
        }
    }
}

impl Power<Read> {
    /// Create new power output pin state reader.
    ///
    /// This uses the device file directly.
    pub fn read_only() -> std::io::Result<Self> {
        Ok(Self {
            power_request: Attribute::readable(PWR_PATH)?,
            reg: Regulators::new()?,
        })
    }
}

impl Power<ReadWrite> {
    /// Create new power output pin state handler.
    ///
    /// This uses the device file directly.
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            power_request: Attribute::read_and_writable(PWR_PATH)?,
            reg: Regulators::new()?,
        })
    }
}

impl<A: access::Access + access::Write + Send> Power<A> {
    /// Set power request.
    ///
    /// Kernel drivers may affect whether power state actually changes.
    fn set_power_request(&mut self, state: State) -> std::io::Result<()> {
        self.power_request.write(state.to_bytes())
    }
}

impl<A: access::Access + access::Read + Send> Power<A> {
    /// Get current power request.
    ///
    /// This may not necessarily reflect the true power state if there are other users of the 5V
    /// output voltage regulator or bus voltage regulator.
    pub fn power_request(&mut self) -> std::io::Result<State> {
        State::from_str(self.power_request.read_to_string()?.trim_end())
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "Invalid power request state"))
    }

    /// Check if 5V power output is enabled.
    pub fn is_powered(&mut self) -> std::io::Result<bool> {
        self.reg.power.is_enabled()
    }

    /// Check if bus power is enabled.
    pub fn is_bus_powered(&mut self) -> std::io::Result<bool> {
        self.reg.bus.is_enabled()
    }

    /// Read power state.
    ///
    /// This returns actual combined state of the voltage regulators.
    pub fn state(&mut self) -> std::io::Result<State> {
        match (self.is_bus_powered()?, self.is_powered()?) {
            (true, true) => Ok(State::Bus),
            (true, false) => Err(io::Error::other(
                "Invalid power state, bus is powered but 5V out is off",
            )),
            (false, true) => Ok(State::Out),
            (false, false) => Ok(State::Off),
        }
    }
}

/// Power state request failed.
#[derive(Debug, Error)]
pub enum PowerStateRequestError {
    /// IO error while setting state.
    #[error("IO error while setting state: {0}")]
    IoWhileSetting(io::Error),
    /// IO error while checking state.
    ///
    /// The state may have changed.
    #[error("IO error while checking state: {0}")]
    IoWhileChecking(io::Error),
    /// Power state did not change as expected.
    #[error("Unexpected power state change: {0:?}")]
    UnexpectedChange(State),
}

impl From<PowerStateRequestError> for io::Error {
    fn from(error: PowerStateRequestError) -> Self {
        use PowerStateRequestError::*;
        match error {
            IoWhileSetting(error) => error,
            IoWhileChecking(error) => error,
            UnexpectedChange(state) => {
                io::Error::other(format!("Unexpected state change to {state:?}"))
            }
        }
    }
}

/// Result for power state request.
///
/// To know whether the state was taken into use or if some driver kept the power on.
#[derive(Debug, Clone, Copy)]
pub enum StateInUse {
    /// Requested state is in use.
    Expected,
    /// Bus is powered by something else and cannot be turned off.
    BusInUse,
    /// 5V out is powered by something else and cannot be turned off.
    OutInUse,
}

impl<A: access::Access + access::Read + access::Write + Send> Power<A> {
    /// Request power state and check the resulting state.
    ///
    /// Kernel drivers may affect the result.
    pub fn request_state(&mut self, state: State) -> Result<StateInUse, PowerStateRequestError> {
        use PowerStateRequestError::*;
        self.set_power_request(state).map_err(IoWhileSetting)?;
        let new_state = self.state().map_err(IoWhileChecking)?;

        use State::*;
        use StateInUse::*;
        match (state, new_state) {
            (Off, Bus) | (Out, Bus) => Ok(BusInUse),
            (Off, Out) => Ok(OutInUse),
            _ if state == new_state => Ok(Expected),
            _ => Err(UnexpectedChange(new_state)),
        }
    }
}
