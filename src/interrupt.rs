// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

// TODO: Remove the legacy code paths once the new driver is in a release.

//! Interrupt pin handling.
use std::fs::File;
use std::io::{self, ErrorKind, Seek};
use std::path::Path;
use std::time::Duration;
use tokio::{
    io::{unix::AsyncFd, Interest},
    time::sleep,
};

// TODO: Drop the old path once the new driver is in a release.
const INT_PATH: &str = "/sys/devices/platform/yft_pogo_pin/int_state";
const OLD_INT_PATH: &str = "/sys/class/yft_pogo_pin/yft_pogo_pin_int_state";

const INTEREST: Interest = Interest::READABLE.add(Interest::PRIORITY);

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

enum InterruptInternal {
    Epoll(AsyncFd<File>),
    Wait(File),
}

/// Interrupt pin state handling.
pub struct Interrupt {
    internal: InterruptInternal,
}

impl Interrupt {
    /// Create new interrupt pin state handler.
    ///
    /// This uses the device file directly.
    pub fn new() -> std::io::Result<Self> {
        let path = Path::new(INT_PATH);
        Ok(Self {
            internal: if path.exists() {
                InterruptInternal::Epoll(AsyncFd::new(File::open(path)?)?)
            } else {
                InterruptInternal::Wait(File::open(OLD_INT_PATH)?)
            },
        })
    }

    /// Returns the current int pin state.
    pub fn state(&mut self) -> std::io::Result<IntState> {
        let file = match &mut self.internal {
            InterruptInternal::Epoll(async_fd) => async_fd.get_mut(),
            InterruptInternal::Wait(file) => file,
        };
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
            match &self.internal {
                InterruptInternal::Epoll(file) => {
                    let mut guard = file.ready(INTEREST).await?;
                    guard.clear_ready();
                }
                InterruptInternal::Wait(_) => {
                    sleep(SLEEPING_DURATION).await;
                }
            }
        }
        Ok(())
    }
}
