// Copyright (c) 2026 Jolla Mobile Ltd

//! TOH communication.
//!
//! Uses I²C and GPIO to talk with TOH.

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
use tokio::time::sleep;

pub(crate) mod paths {
    pub(crate) const I2C_PATH: &str = "/dev/i2c-0";
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
    /// Requires root access and thus is mainly only good for the daemon.
    pub fn new() -> io::Result<Self> {
        // TODO: Handle also the buffer chip power
        let mut pwr = Power::new()?;
        pwr.set_power(false)?;
        Ok(Self {
            id: Id::new()?,
            int: Interrupt::new()?,
            i2c: I2cDev::toh_dev()?,
            pwr,
            _state: PhantomData,
        })
    }

    /// Wait for connecting TOH.
    ///
    /// Returns immediately if TOH is already connected.
    pub async fn wait_connect(mut self) -> io::Result<BackCover<state::Attached>> {
        self.int.watch(IntState::Low).await?;
        let BackCover {
            id, i2c, int, pwr, ..
        } = self;
        Ok(BackCover {
            id,
            i2c,
            int,
            pwr,
            _state: PhantomData,
        })
    }
}

impl BackCover<state::Attached> {
    /// Power up the connector and return BackCover in new state where the chip can be read.
    ///
    /// Returns an error if TOH is not present.
    /// Returns Ok(None) if TOH is not of a supported type.
    pub async fn power_up(mut self) -> io::Result<Variant> {
        self.pwr.set_power(true)?;
        // TODO: We should probably wait a bit here.
        let adc = self.read_adc().await?;
        let BackCover {
            id,
            i2c,
            int,
            mut pwr,
            ..
        } = self;
        // TODO: Is there a better way to represent this so we don't need to spell out these all?
        match adc.identify() {
            TohId::R10k => Ok(Variant::With256BBlocks(BackCover {
                id,
                i2c,
                int,
                pwr,
                _state: PhantomData::<state::Present256BBlocks>,
            })),
            TohId::R15k => Ok(Variant::With64kBBlocks(BackCover {
                id,
                i2c,
                int,
                pwr,
                _state: PhantomData::<state::Present64kBBlocks>,
            })),
            _ => {
                // TODO: Distinguish between attached TOH and detached TOH.
                pwr.set_power(false)?;
                Ok(Variant::Attached(BackCover {
                    id,
                    i2c,
                    int,
                    pwr,
                    _state: PhantomData::<state::Attached>,
                }))
            }
        }
    }
}

impl<P: state::State + std::marker::Send> BackCover<P> {
    /// Read int state.
    pub fn read_int_state(&mut self) -> io::Result<IntState> {
        self.int.state()
    }

    /// Read ADC.
    ///
    /// Reads the Id pin multiple times and returns the median value.
    pub async fn read_adc(&mut self) -> io::Result<AdcValue> {
        let mut values = Vec::new();
        for _ in 0..5 {
            values.push(self.id.read()?);
            sleep(Duration::from_millis(10)).await;
        }
        values.sort();
        Ok(values[2])
    }
}

/// Trait for power_down.
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
            ..
        } = self;
        pwr.set_power(false)?;
        Ok(BackCover {
            id,
            i2c,
            int,
            pwr,
            _state: PhantomData,
        })
    }

    fn power_down_boxed(self: Box<Self>) -> io::Result<BackCover<state::Detached>> {
        self.power_down()
    }
}

/// Trait for wait_disconnect method.
#[async_trait]
pub trait WaitDisconnect {
    /// Wait for disconnection of TOH.
    ///
    /// Returns immediately if TOH is already disconnected.
    async fn wait_disconnect(mut self) -> io::Result<BackCover<state::Detached>>;

    // NB: Just to workaround some inconveniences in Rust.
    /// Wait for disconnection of TOH for boxed instance.
    ///
    /// See also wait_disconnect.
    async fn wait_disconnect_boxed(mut self: Box<Self>) -> io::Result<BackCover<state::Detached>>;
}

#[async_trait]
impl<P: state::State + std::marker::Send> WaitDisconnect for BackCover<P> {
    async fn wait_disconnect(mut self) -> io::Result<BackCover<state::Detached>> {
        self.int.watch(IntState::High).await?;
        let BackCover {
            id, i2c, int, pwr, ..
        } = self;
        Ok(BackCover {
            id,
            i2c,
            int,
            pwr,
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
    type Error = io::Error;
    async fn is_present(&mut self) -> Result<bool, Self::Error> {
        if self.read_int_state()? == IntState::Low {
            Ok(self.read_adc().await?.is_toh_present())
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
            // Set target address, this won't actually do anything on the bus.
            self.i2c.set_target_address(address)?;
            // Set data address to zero which can fail if there is no such chip.
            match self.i2c.write_all(&[0]) {
                Ok(_) => {
                    self.i2c.read_exact(&mut buf)?;
                    result.extend(buf);
                }
                Err(error) if error.raw_os_error() == Some(libc::ENXIO) && address != 0x50 => {
                    // No more blocks, all read.
                    return Ok(result);
                }
                Err(error) => {
                    // Something else went wrong.
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
    Io(#[from] std::io::Error),
    /// Parsing error happened.
    #[error("Parsing error: {0}")]
    Parse(#[from] ParseError),
}

#[async_trait]
impl Detect for BackCover<state::Present256BBlocks> {
    type Error = DetectionError;
    async fn detect(&mut self) -> Result<Option<Info>, Self::Error> {
        let content = self.read_chip()?;
        Ok(Some(Info::parse_from_bytes(&content)?))
    }
}

#[async_trait]
impl Detect for BackCover<state::Present64kBBlocks> {
    type Error = DetectionError;
    async fn detect(&mut self) -> Result<Option<Info>, Self::Error> {
        // TODO: We need to consider how we make the ID value detection so robust that we don't
        // accidentally rewrite the first byte on those 8-bit memory chips,
        // or alternatively we need to do this in a way that does not result in overwrites.
        Ok(None) // TODO: Implement reading for Present64kBBlocks too
    }
}
