// Copyright (c) 2026 Jolla Mobile Ltd

//! Power output pin handling.
use crate::back_cover::paths::PWR_PATH;
use std::fs::File;
use std::io::{self, ErrorKind, Seek, Write as _};
use std::marker::{PhantomData, Send};

mod access {
    pub trait Access {}
    pub trait Read {}
    pub trait Write {}
}

pub struct Read {}
impl access::Access for Read {}
impl access::Read for Read {}

pub struct ReadWrite {}
impl access::Access for ReadWrite {}
impl access::Read for ReadWrite {}
impl access::Write for ReadWrite {}

/// Power output pin state handling.
pub struct Power<A: access::Access + Send> {
    file: File,
    _access: PhantomData<A>,
}

impl Power<Read> {
    /// Create new power output pin state reader.
    ///
    /// This uses the device file directly.
    pub fn read_only() -> std::io::Result<Self> {
        Ok(Self {
            file: File::open(PWR_PATH)?,
            _access: PhantomData,
        })
    }
}

impl Power<ReadWrite> {
    /// Create new power output pin state handler.
    ///
    /// This uses the device file directly.
    pub fn new() -> std::io::Result<Self> {
        // TODO: Could we lock the file so that other processes cannot change it?
        Ok(Self {
            file: File::options().read(true).write(true).open(PWR_PATH)?,
            _access: PhantomData,
        })
    }
}

impl<A: access::Access + access::Write + Send> Power<A> {
    /// Set power output.
    pub fn set_power(&mut self, enabled: bool) -> std::io::Result<()> {
        self.file.rewind()?;
        self.file.write_all(if enabled { b"1" } else { b"0" })?;
        self.file.flush()?;
        Ok(())
    }
}

impl<A: access::Access + access::Read + Send> Power<A> {
    /// Check if power output is enabled.
    pub fn is_powered(&mut self) -> std::io::Result<bool> {
        self.file.rewind()?;
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
