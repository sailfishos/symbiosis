// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! Interrupt pin handling.
use crate::back_cover::paths::INT_PATH;
use std::fs::File;
use std::io::{self, ErrorKind, Seek};
use tokio::io::{unix::AsyncFd, Interest};

const INTEREST: Interest = Interest::READABLE.add(Interest::PRIORITY);

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
    file: AsyncFd<File>,
}

impl Interrupt {
    /// Create new interrupt pin state handler.
    ///
    /// This uses the device file directly.
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            file: AsyncFd::new(File::open(INT_PATH)?)?,
        })
    }

    /// Returns the current int pin state.
    pub fn state(&mut self) -> std::io::Result<IntState> {
        self.file.get_mut().rewind()?;
        use IntState::*;
        match io::read_to_string(self.file.get_mut())?
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
            let _guard = self.file.ready(INTEREST).await?;
        }
        Ok(())
    }
}
