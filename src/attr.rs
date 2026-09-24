// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! Helpers to deal with sysfs attributes.
//!
//! These always open and close the file when reading or writing.

use std::fs::File;
use std::io::{self, Read as _, Write as _};
use std::marker::{PhantomData, Send};
use std::path::{Path, PathBuf};

pub(crate) mod access {
    pub trait Access {}
    pub trait Read {}
    pub trait Write {}
}

#[derive(Clone, Debug)]
pub struct Read {}
impl access::Access for Read {}
impl access::Read for Read {}

#[derive(Clone, Debug)]
pub struct Write {}
impl access::Access for Write {}
impl access::Write for Write {}

#[derive(Clone, Debug)]
pub struct ReadWrite {}
impl access::Access for ReadWrite {}
impl access::Read for ReadWrite {}
impl access::Write for ReadWrite {}

/// Small helper to aid dealing with sysfs attributes.
#[derive(Clone, Debug)]
pub(crate) struct Attribute<S: access::Access + Send> {
    pub path: PathBuf,
    _dir: PhantomData<S>,
}

impl Attribute<Read> {
    pub fn readable<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let attr = Self {
            path: path.as_ref().to_path_buf(),
            _dir: PhantomData,
        };
        attr.open_readable()?;
        Ok(attr)
    }
}

impl<S: access::Access + access::Read + Send> Attribute<S> {
    fn open_readable(&self) -> io::Result<File> {
        File::options().create(false).read(true).open(&self.path)
    }

    pub fn read_to_string(&self) -> io::Result<String> {
        self.open_readable().and_then(|mut file| {
            let mut buf = String::new();
            file.read_to_string(&mut buf)?;
            Ok(buf)
        })
    }
}

impl Attribute<Write> {
    pub fn writable<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let attr = Self {
            path: path.as_ref().to_path_buf(),
            _dir: PhantomData,
        };
        attr.open_writable()?;
        Ok(attr)
    }
}

impl<S: access::Access + access::Write + Send> Attribute<S> {
    fn open_writable(&self) -> io::Result<File> {
        File::options().create(false).write(true).open(&self.path)
    }

    pub fn write(&self, content: &[u8]) -> io::Result<()> {
        self.open_writable().and_then(|mut file| {
            file.write_all(content)?;
            file.sync_all()?;
            Ok(())
        })
    }
}

impl Attribute<ReadWrite> {
    pub fn read_and_writable<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let attr = Self {
            path: path.as_ref().to_path_buf(),
            _dir: PhantomData,
        };
        attr.open_readable()?;
        attr.open_writable()?;
        Ok(attr)
    }
}
