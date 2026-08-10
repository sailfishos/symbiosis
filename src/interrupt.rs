// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! Interrupt pin handling.
use crate::back_cover::paths::INT_PATH;
use std::fs::File;
use std::io::{self, ErrorKind, Seek};
use std::time::Duration;
use tokio::time::sleep;

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
    file: File,
}

impl Interrupt {
    /// Create new interrupt pin state handler.
    ///
    /// This uses the device file directly.
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            file: File::open(INT_PATH)?,
        })
    }

    /// Returns the current int pin state.
    pub fn state(&mut self) -> std::io::Result<IntState> {
        self.file.rewind()?;
        use IntState::*;
        match io::read_to_string(&self.file)?
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
        // TODO: Replace with proper implementation.
        // Currently we just check periodically because there is no implementation to do this
        // through epoll. Once we have that we can replace this. Or use the other method already in
        // the kernel when someone writes the user space part for that.
        while self.state()? != expected {
            sleep(SLEEPING_DURATION).await;
        }
        Ok(())
    }
}
