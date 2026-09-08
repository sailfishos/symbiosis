// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! TOH communication.
//!
//! Uses I²C and GPIO to talk with TOH.

use crate::bus::I2cBus;
use crate::i2cdev::I2cDev;
use crate::id::{AdcValue, Id, TohId};
use crate::interrupt::{IntState, Interrupt};
use crate::power::{self, Power};
use crate::toh::*;
use async_trait::async_trait;
use std::io::{self, Read, Write};
use std::marker::PhantomData;
use std::time::Duration;
use thiserror::Error;
use tokio::time::{interval, sleep, MissedTickBehavior};

// Difference that is considered acceptable for attached TOH ID pin ADC values.
const ACCEPTED_ADC_DIFFERENCE: f64 = 0.005; // 0.5 %

pub(crate) mod paths {
    pub(crate) const PWR_PATH: &str = "/sys/class/yft_pogo_pin/yft_pogo_pin_5v_out_state";
    pub(crate) const ADC_PATH: &str = "/sys/class/yft_pogo_pin/yft_pogo_pin_adc_value";
    pub(crate) const INT_PATH: &str = "/sys/class/yft_pogo_pin/yft_pogo_pin_int_state";
}

mod state {
    pub trait State {}

    /// TOH has not been been detected.
    pub struct Detached {}

    /// TOH has been detected but not identified.
    pub struct Attached {}

    /// Memory chip is readable with 8-bit data addressing.
    ///
    /// This is the variant that has a memory chip with small blocks (up to 256 bytes).
    pub struct Present256BBlocks {}

    /// Memory chip is readable with 16-bit data addressing.
    ///
    /// This is the variant that has a memory chip with large (up to 64 kibibyte) blocks.
    pub struct Present64kBBlocks {}

    impl State for Detached {}
    impl State for Attached {}
    impl State for Present256BBlocks {}
    impl State for Present64kBBlocks {}
}

/// TOH implementation that talks via I²C and GPIO.
pub struct BackCover<P: state::State + std::marker::Send> {
    id: Id,
    i2c: I2cDev,
    int: Interrupt,
    pwr: Power<power::ReadWrite>,
    bus: I2cBus,
    _state: PhantomData<P>,
}

/// TOH variants.
pub enum Variant {
    /// Unknown type of TOH attached.
    Attached(BackCover<state::Attached>),
    /// TOH with memory chip containing up to 256 byte blocks.
    With256BBlocks(BackCover<state::Present256BBlocks>),
    /// TOH with memory chip containing up to 64 kiB blocks.
    With64kBBlocks(BackCover<state::Present64kBBlocks>),
}

impl BackCover<state::Detached> {
    /// Create new instance from device files.
    ///
    /// Requires _root_ access and thus is mainly only good for the daemon.
    pub fn new() -> io::Result<Self> {
        let bus = I2cBus::toh_bus()?;
        // TODO: Handle also the buffer chip power
        let mut pwr = Power::new()?;
        pwr.set_power(false)?;
        Ok(Self {
            id: Id::new()?,
            int: Interrupt::new()?,
            i2c: bus.i2c_dev()?,
            pwr,
            bus,
            _state: PhantomData,
        })
    }

    /// Wait for connecting TOH.
    ///
    /// Returns immediately if TOH is already connected.
    pub async fn wait_connect(mut self) -> io::Result<BackCover<state::Attached>> {
        self.int.watch(IntState::Low).await?;
        let BackCover {
            id,
            i2c,
            int,
            pwr,
            bus,
            ..
        } = self;
        Ok(BackCover {
            id,
            i2c,
            int,
            pwr,
            bus,
            _state: PhantomData,
        })
    }
}

/// Error when identifying back cover.
#[derive(Debug, Error)]
pub enum IdentificationError {
    /// IO error while identifying cover.
    ///
    /// This can happen when INT pin state is read again, or when power is enabled or disabled.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    /// IO error while reading ADC.
    #[error("IO error while reading ADC: {0}")]
    AdcIo(io::Error),
    /// Cover got removed before identification.
    #[error("Cover disconnected")]
    Disconnected,
    /// Badly seated back cover detected.
    ///
    /// In that case it is probably best to try again until cover is properly in place or removed.
    #[error("Cover is badly seated")]
    BadlySeated,
    /// ID resistor is not detected.
    ///
    /// In other words INT was low but ADC read as disconnected. This may be an intermittent error.
    #[error("ID resistor was not detected")]
    IdResistorNotDetected,
}

impl From<AdcReadError> for IdentificationError {
    fn from(error: AdcReadError) -> Self {
        use IdentificationError::*;
        match error {
            AdcReadError::Io(error) => AdcIo(error),
            AdcReadError::Inconsistent => BadlySeated,
        }
    }
}

impl BackCover<state::Attached> {
    /// Power up the connector and return [`BackCover`] in a new state in which the chip can be
    /// read.
    ///
    /// Returns an error if TOH is not present or cannot be identified.
    pub async fn power_up(mut self) -> Result<Variant, IdentificationError> {
        // TODO: Could we have some guard type for power?
        self.pwr.set_power(true)?;
        let adc = self.read_adc().await?;
        if self.read_int_state()? != IntState::Low {
            // INT got disconnected => TOH is no longer present.
            self.pwr.set_power(false)?;
            return Err(IdentificationError::Disconnected);
        }

        let BackCover {
            id,
            i2c,
            int,
            mut pwr,
            bus,
            ..
        } = self;
        // TODO: Is there a better way to represent this so we don't need to spell out these all?
        match adc.identify() {
            TohId::R10k => Ok(Variant::With256BBlocks(BackCover {
                id,
                i2c,
                int,
                pwr,
                bus,
                _state: PhantomData::<state::Present256BBlocks>,
            })),
            TohId::R15k => Ok(Variant::With64kBBlocks(BackCover {
                id,
                i2c,
                int,
                pwr,
                bus,
                _state: PhantomData::<state::Present64kBBlocks>,
            })),
            TohId::NotPresent => {
                // INT pin claims that there is a cover but no resistor was found.
                pwr.set_power(false)?;
                Err(IdentificationError::IdResistorNotDetected)
            }
            TohId::Unknown => {
                // Unsupported type.
                pwr.set_power(false)?;
                Ok(Variant::Attached(BackCover {
                    id,
                    i2c,
                    int,
                    pwr,
                    bus,
                    _state: PhantomData::<state::Attached>,
                }))
            }
        }
    }
}

/// Error during reading ADC multiple times.
#[derive(Debug, Error)]
pub enum AdcReadError {
    /// IO error while reading ADC.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    /// Inconsistent results.
    #[error("Inconsistent readings")]
    Inconsistent,
}

impl<P: state::State + std::marker::Send> BackCover<P> {
    /// Read int state.
    pub fn read_int_state(&mut self) -> io::Result<IntState> {
        self.int.state()
    }

    /// Read ADC.
    ///
    /// Reads the Id pin multiple times and returns the median value.
    ///
    /// If the ADC does not give consistent values, this returns an error.
    pub async fn read_adc(&mut self) -> Result<AdcValue, AdcReadError> {
        let mut values = [AdcValue::default(); 5];
        let mut interval = interval(Duration::from_millis(100));
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        for value in &mut values {
            interval.tick().await;
            *value = self.id.read()?;
        }
        log::debug!("ADC readings: {values:?}");
        values.sort();
        let [min, _, median, _, max] = values;
        // If minimum and maximum differ too much, we consider ADC readings inconsistent.
        if f64::from(max - min) / f64::from(median) > ACCEPTED_ADC_DIFFERENCE {
            Err(AdcReadError::Inconsistent)
        } else {
            Ok(median)
        }
    }
}

/// Trait for [`power_down`](Self::power_down) method.
pub trait PowerDown {
    /// Power down TOH.
    fn power_down(self) -> io::Result<BackCover<state::Detached>>;

    // NB: Just to workaround some inconveniences in Rust.
    /// Power down TOH.
    fn power_down_boxed(self: Box<Self>) -> io::Result<BackCover<state::Detached>>;
}

impl<P: state::State + std::marker::Send> PowerDown for BackCover<P> {
    fn power_down(self) -> io::Result<BackCover<state::Detached>> {
        let BackCover {
            id,
            i2c,
            int,
            mut pwr,
            bus,
            ..
        } = self;
        pwr.set_power(false)?;
        Ok(BackCover {
            id,
            i2c,
            int,
            pwr,
            bus,
            _state: PhantomData,
        })
    }

    fn power_down_boxed(self: Box<Self>) -> io::Result<BackCover<state::Detached>> {
        self.power_down()
    }
}

/// Trait for [`wait_disconnect`](Self::wait_disconnect) method.
#[async_trait]
pub trait WaitDisconnect {
    /// Wait for disconnection of TOH.
    ///
    /// Returns immediately if TOH is already disconnected.
    async fn wait_disconnect(mut self) -> io::Result<BackCover<state::Detached>>;

    // NB: Just to workaround some inconveniences in Rust.
    /// Wait for disconnection of TOH for boxed instance.
    ///
    /// See also [`wait_disconnect`](Self::wait_disconnect).
    async fn wait_disconnect_boxed(mut self: Box<Self>) -> io::Result<BackCover<state::Detached>>;
}

#[async_trait]
impl<P: state::State + std::marker::Send> WaitDisconnect for BackCover<P> {
    async fn wait_disconnect(mut self) -> io::Result<BackCover<state::Detached>> {
        loop {
            self.int.watch(IntState::High).await?;
            // Check that the cover was actually removed by reading ADC one more time.
            // If ID pin happens to get disconnected only after this check, we will anyway loop back
            // over, read INT pin one more time and check ID pin again.
            if !self.id.read()?.is_toh_present() {
                // TOH is disconnected
                break;
            } else {
                // TODO: Reporting interrupts
                sleep(Duration::from_millis(100)).await;
            }
        }
        let BackCover {
            id,
            i2c,
            int,
            pwr,
            mut bus,
            ..
        } = self;
        bus.remove_all_targets()?;
        Ok(BackCover {
            id,
            i2c,
            int,
            pwr,
            bus,
            _state: PhantomData,
        })
    }

    async fn wait_disconnect_boxed(mut self: Box<Self>) -> io::Result<BackCover<state::Detached>> {
        self.wait_disconnect().await
    }
}

#[async_trait]
impl<P: state::State + std::marker::Send> IsPowered for BackCover<P> {
    type Error = io::Error;

    async fn is_powered(&mut self) -> Result<bool, Self::Error> {
        self.pwr.is_powered()
    }
}

#[async_trait]
impl<P: state::State + std::marker::Send> IsPresent for BackCover<P> {
    type Error = AdcReadError;

    async fn is_present(&mut self) -> Result<bool, Self::Error> {
        if self.read_int_state()? == IntState::Low {
            // This checks INT state again to ensure it remained low after reading ADC
            Ok(self.read_adc().await?.is_toh_present() && self.read_int_state()? == IntState::Low)
        } else {
            Ok(false)
        }
    }
}

impl BackCover<state::Present256BBlocks> {
    /// Use I²C to read the chip header and payload content into a vector.
    pub fn read_chip(&mut self) -> io::Result<Vec<u8>> {
        let mut result = Vec::new();
        let mut buf = [0; 256];
        for address in 0x50..0x60 {
            // Since I3C wrapper driver for I²C cannot be used without configuring devices first, we
            // are creating the devices on the bus before using them.
            let target = self.bus.add_target(address, None)?;
            // Set target address, this won't actually do anything on the bus.
            self.i2c.set_target_address(address.into())?;
            // Set data address to zero which can fail if there is no such chip.
            match self.i2c.write_all(&[0]) {
                Ok(_) => {
                    self.i2c.read_exact(&mut buf)?;
                    result.extend(buf);
                    target.remove()?;
                }
                Err(error)
                    if (error.kind() == io::ErrorKind::NotFound
                        || error.raw_os_error() == Some(libc::ENXIO))
                        && address != 0x50 =>
                {
                    // No more blocks, all read.
                    target.remove()?;
                    return Ok(result);
                }
                Err(error) => {
                    // Something else went wrong.
                    let _ = target.remove();
                    return Err(error);
                }
            }
        }
        Ok(result)
    }
}

/// Error during TOH detection.
#[derive(Debug, Error)]
pub enum DetectionError {
    /// IO error happened.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    /// Parsing error happened.
    #[error("Parsing error: {0}")]
    Parse(#[from] ParseError),
}

#[async_trait]
impl Detect for BackCover<state::Present256BBlocks> {
    type Error = DetectionError;

    async fn detect(&mut self) -> Result<Option<Info>, Self::Error> {
        // Check ID pin and INT pin one more time to see that TOH is still there
        if self.id.read()?.is_toh_present() && self.read_int_state()? == IntState::Low {
            let content = self.read_chip()?;
            Ok(Some(Info::parse_from_bytes(&content)?))
        } else {
            Ok(None)
        }
    }
}

#[async_trait]
impl Detect for BackCover<state::Present64kBBlocks> {
    type Error = DetectionError;

    async fn detect(&mut self) -> Result<Option<Info>, Self::Error> {
        // TODO: We need to consider how we make the ID value detection so robust that we don't
        // accidentally rewrite the first byte on those 8-bit memory chips,
        // or alternatively we need to do this in a way that does not result in overwrites.
        // TODO: Implement reading for Present64kBBlocks too
        Err(DetectionError::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            "Not implemented yet",
        )))
    }
}
