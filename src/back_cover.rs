// Copyright (c) 2026 Jolla Mobile Ltd

//! TOH communication.
//!
//! Uses I²C and GPIO to talk with the TOH.

use crate::i2cdev::I2CDev;
use crate::id::{AdcValue, Id, TohId};
use crate::interrupt::{IntState, Interrupt};
use crate::power::Power;
use crate::toh::*;
use async_trait::async_trait;
use std::io::{self, ErrorKind, Read, Write};
use std::marker::PhantomData;

pub mod paths {
    // TODO: Get path properly to avoid accidentally writing something unintended
    pub const I2C_PATH: &str = "/dev/i2c-0";
    pub const PWR_PATH: &str = "/sys/class/yft_pogo_pin/yft_pogo_pin_5v_out_state";
    pub const ADC_PATH: &str = "/sys/class/yft_pogo_pin/yft_pogo_pin_adc_value";
    pub const INT_PATH: &str = "/sys/class/yft_pogo_pin/yft_pogo_pin_int_state";
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
    i2c: I2CDev,
    int: Interrupt,
    pwr: Power,
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
        // TODO: This should turn off the power here so we always start at that state.
        Ok(Self {
            id: Id::new()?,
            int: Interrupt::new()?,
            i2c: I2CDev::new(paths::I2C_PATH)?,
            pwr: Power::new()?,
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
    pub fn power_up(mut self) -> io::Result<Variant> {
        self.pwr.set_power(true)?;
        // TODO: We should probably wait a bit here.
        let adc = self.read_adc()?;
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
    pub fn read_adc(&mut self) -> io::Result<AdcValue> {
        let mut values = Vec::new();
        for _ in 0..5 {
            // TODO: This could wait a little between reads but that requires making the method async.
            values.push(self.id.read()?);
        }
        values.sort();
        Ok(values[2])
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

impl<P: state::State + std::marker::Send> IsPowered for BackCover<P> {
    fn is_powered(&mut self) -> io::Result<bool> {
        self.pwr.is_powered()
    }
}

impl<P: state::State + std::marker::Send> IsPresent for BackCover<P> {
    fn is_present(&mut self) -> io::Result<bool> {
        if self.read_int_state()? == IntState::Low {
            Ok(self.read_adc()?.is_toh_present())
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
            match self.i2c.set_target_address(address) {
                Ok(_) => {
                    // Set data address to zero
                    self.i2c.write_all(&[0])?;

                    self.i2c.read_exact(&mut buf)?;
                    result.extend(buf);
                }
                Err(error) => {
                    return if let ErrorKind::NotFound = error.kind() {
                        // No more blocks, all read.
                        Ok(result)
                    } else {
                        // Something else went wrong.
                        Err(error)
                    };
                }
            }
        }
        Ok(result)
    }
}

impl Detect for BackCover<state::Present256BBlocks> {
    fn detect(&mut self) -> Result<Option<Info>, DetectionError> {
        let content = self.read_chip()?;
        Ok(Some(Info::parse_from_bytes(&content)?))
    }
}

// TODO: Implement the functions for Present64kBBlocks too
