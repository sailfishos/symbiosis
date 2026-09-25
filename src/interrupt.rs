// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

// TODO: Remove the legacy code paths once the new driver is in a release.

//! Interrupt pin handling.
use std::fs::File;
use std::io::{self, ErrorKind, Seek};
#[cfg(not(feature = "new_kernel_api"))]
use std::time::Duration;
#[cfg(feature = "new_kernel_api")]
use tokio::io::{unix::AsyncFd, Interest};
#[cfg(not(feature = "new_kernel_api"))]
use tokio::time::sleep;

#[cfg(feature = "new_kernel_api")]
const INT_PATH: &str = "/sys/devices/platform/yft_pogo_pin/int_state";
#[cfg(not(feature = "new_kernel_api"))]
const INT_PATH: &str = "/sys/class/yft_pogo_pin/yft_pogo_pin_int_state";

#[cfg(feature = "new_kernel_api")]
const INTEREST: Interest = Interest::READABLE.add(Interest::PRIORITY);

#[cfg(not(feature = "new_kernel_api"))]
const SLEEPING_DURATION: Duration = Duration::from_millis(100);

/// Interrupt pin state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntState {
    Low,
    High,
}

impl IntState {
    /// Returns the other state.
    pub fn toggle(self) -> Self {
        use IntState::*;
        match self {
            Low => High,
            High => Low,
        }
    }
}

/// Interrupt pin state handling.
pub struct Interrupt {
    #[cfg(feature = "new_kernel_api")]
    file: AsyncFd<File>,
    #[cfg(not(feature = "new_kernel_api"))]
    file: File,
}

impl Interrupt {
    /// Create new interrupt pin state handler.
    ///
    /// This uses the device file directly.
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            #[cfg(feature = "new_kernel_api")]
            file: AsyncFd::new(File::open(INT_PATH)?)?,
            #[cfg(not(feature = "new_kernel_api"))]
            file: File::open(INT_PATH)?,
        })
    }

    /// Returns the current int pin state.
    pub fn state(&mut self) -> std::io::Result<IntState> {
        #[cfg(feature = "new_kernel_api")]
        let file = self.file.get_mut();
        #[cfg(not(feature = "new_kernel_api"))]
        let file = &mut self.file;
        file.rewind()?;
        use IntState::*;
        match io::read_to_string(file)?
            .trim_end()
            .parse::<u8>()
            .map_err(|err| io::Error::new(ErrorKind::InvalidData, err.to_string()))?
        {
            0 => Ok(Low),
            1 => Ok(High),
            _ => Err(io::Error::new(
                ErrorKind::InvalidData,
                "Invalid INT pin state",
            )),
        }
    }

    /// Asynchronously watch for state to change to expected on interrupt.
    pub async fn watch(&mut self, expected: IntState) -> std::io::Result<()> {
        while self.state()? != expected {
            #[cfg(feature = "new_kernel_api")]
            {
                let mut guard = self.file.ready(INTEREST).await?;
                guard.clear_ready();
            }
            #[cfg(not(feature = "new_kernel_api"))]
            sleep(SLEEPING_DURATION).await;
        }
        Ok(())
    }
}
