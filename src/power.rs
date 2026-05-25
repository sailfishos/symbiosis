// Copyright (c) 2026 Jolla Mobile Ltd

//! Power output pin handling.
use std::fs::File;
use std::io::{self, ErrorKind, Seek, SeekFrom, Write};

// TODO: Could we lock the file so that other processes cannot change it?
const PWR_PATH: &str = "/sys/class/yft_pogo_pin/yft_pogo_pin_5v_out_state";

/// Power output pin state handling.
pub struct Power {
    file: File,
}

impl Power {
    /// Create new power output pin state handler.
    ///
    /// This uses the device file directly.
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            file: File::create(PWR_PATH)?,
        })
    }

    /// Set power output.
    pub fn set_power(&mut self, enabled: bool) -> std::io::Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(if enabled { b"1" } else { b"0" })?;
        self.file.flush()?;
        Ok(())
    }

    /// Check if power output is enabled.
    pub fn is_powered(&mut self) -> std::io::Result<bool> {
        self.file.seek(SeekFrom::Start(0))?;
        match io::read_to_string(&self.file)?
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
