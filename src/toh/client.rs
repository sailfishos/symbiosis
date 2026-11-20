// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! Access to TOH through `org.sailfishos.tohd1`.

use super::*;
use crate::{dbus::TohProxy, errors::ExtraValueConversionError, i2cdev::I2cDev, power::Power};
use async_trait::async_trait;
use futures::future::BoxFuture;
use std::collections::BTreeMap;
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream as StdUnixStream;
use thiserror::Error;
use tokio::net::UnixStream;
use zbus::{
    fdo::{Error as DBusError, PeerProxy},
    Connection,
};

/// Error while fetching TOH info.
#[derive(Debug, Error)]
pub enum FetchingInfoError {
    /// D-Bus error happened.
    #[error("D-Bus error: {0}")]
    DBus(#[from] DBusError),
    /// Conversion error happened.
    #[error("Conversion error: {0}")]
    Conversion(#[from] ExtraValueConversionError),
}

/// Error while trying to access i2c-dev device.
#[derive(Debug, Error)]
pub enum AccessError<T> {
    /// Error while borrowing the device.
    #[error("Borrowing failed: {0}")]
    Borrow(#[from] BorrowError),
    /// IO error with the device.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    /// Error while returning the device.
    #[error("Returning failed: {1}")]
    Return(T, ReturnError),
}

impl<T> From<(ReturnError, T)> for AccessError<T> {
    fn from((error, value): (ReturnError, T)) -> Self {
        AccessError::Return(value, error)
    }
}

/// Client side implementation.
///
/// Uses D-Bus to communicate with TOH daemon.
#[derive(Debug)]
pub struct Toh<'proxy> {
    toh: TohProxy<'proxy>,
}

impl<'proxy> Toh<'proxy> {
    /// Create new instance.
    pub async fn new() -> Result<Self, zbus::Error> {
        let connection = Connection::system().await?;
        let toh = TohProxy::new(&connection).await?;
        Ok(Self { toh })
    }

    /// Get TohProxy instance.
    pub fn toh_proxy(&self) -> &TohProxy<'proxy> {
        &self.toh
    }

    // Nightly feature async_fn_traits would allow using AsyncFnOnce instead and avoid boxing and
    // pinning.
    /// Access i2c-dev device temporarily.
    pub async fn access_i2c_dev<T, F>(&mut self, f: F) -> Result<T, AccessError<T>>
    where
        F: for<'dev> FnOnce(&'dev mut I2cDev) -> BoxFuture<'dev, T>,
    {
        let fd = self.toh.borrow_i2c_dev_access().await?;
        let mut dev = I2cDev::from_fd(fd.into())?;
        let result = f(&mut dev).await;
        if let Err(error) = self.toh.return_i2c_dev_access().await {
            Err(AccessError::from((error, result)))
        } else {
            Ok(result)
        }
    }

    /// Access i2c-dev device temporarily and select whether to leave power on.
    pub async fn access_i2c_dev_with_power<T, F>(
        &mut self,
        leave_power_on: bool,
        f: F,
    ) -> Result<T, AccessError<T>>
    where
        F: for<'dev> FnOnce(&'dev mut I2cDev) -> BoxFuture<'dev, T>,
    {
        let fd = self
            .toh
            .borrow_i2c_dev_access_with_power(leave_power_on)
            .await?;
        let mut dev = I2cDev::from_fd(fd.into())?;
        let result = f(&mut dev).await;
        if let Err(error) = self.toh.return_i2c_dev_access().await {
            Err(AccessError::from((error, result)))
        } else {
            Ok(result)
        }
    }

    async fn fetch_info(&self) -> Result<Info, FetchingInfoError> {
        // TODO: Make sure all properties have been fetched to avoid many D-Bus calls
        // TODO: How do we check that if the D-Bus object disappears that the info is still
        // current?! Is there some stream we could listen to here?
        Ok(
            Info {
                vendor_id: self.toh.vendor_id().await?,
                product_id: self.toh.product_id().await?,
                schema_version: self.toh.schema_version().await?.into(),
                serial_number: self.toh.serial_number().await?.into(),
                vendor_name: self.toh.vendor_name().await?.into(),
                product_name: self.toh.product_name().await?.into(),
                vendor_website: self.toh.vendor_website().await?.into(),
                product_website: self.toh.product_website().await?.into(),
                leave_power_on: self.toh.leave_power_on().await?.into(),
                power_input_toh: self.toh.power_input_toh().await?.into(),
                extra:
                    self.toh
                        .extra_data()
                        .await?
                        .into_iter()
                        .map(|(key, value)| Ok((key, (&*value).try_into()?)))
                        .collect::<std::result::Result<
                            BTreeMap<String, ExtraValue>,
                            ExtraValueConversionError,
                        >>()?,
            },
        )
    }
}

#[async_trait]
impl<'proxy> IsPowered for &Toh<'proxy> {
    type Error = io::Error;
    async fn is_powered(&mut self) -> Result<bool, Self::Error> {
        Power::read_only()?.is_powered()
    }
}

#[async_trait]
impl<'proxy> IsPresent for &Toh<'proxy> {
    type Error = DBusError;
    async fn is_present(&mut self) -> Result<bool, Self::Error> {
        let peer = PeerProxy::new(
            self.toh.as_ref().connection(),
            self.toh.as_ref().destination().clone().into_owned(),
            self.toh.as_ref().path().clone().into_owned(),
        )
        .await?;
        match peer.ping().await {
            Ok(_) => Ok(true),
            Err(DBusError::UnknownObject(_)) => Ok(false),
            Err(error) => Err(error),
        }
    }
}

#[async_trait]
impl<'proxy> Detect for &Toh<'proxy> {
    type Error = FetchingInfoError;

    async fn detect(&mut self) -> Result<Option<Info>, Self::Error> {
        match self.fetch_info().await {
            Ok(info) => Ok(Some(info)),
            Err(FetchingInfoError::DBus(DBusError::UnknownObject(..))) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// Subscription to TOH interrupts.
///
/// This allows to wait for interrupts on the connected TOH.
#[derive(Debug)]
pub struct InterruptSubscription {
    stream: UnixStream,
}

/// Error while trying to subscribe to interrupts.
#[derive(Debug, Error)]
pub enum SubscribeInterruptsError {
    /// D-Bus error happened.
    #[error("D-Bus error: {0}")]
    DBus(#[from] DBusError),
    /// Subscribing error happened.
    #[error("Subscribe error: {0}")]
    Subscribe(SubscribeError),
    /// IO error with the file descriptor.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

impl From<SubscribeError> for SubscribeInterruptsError {
    fn from(error: SubscribeError) -> Self {
        if let SubscribeError::ZBus(error) = error {
            Self::DBus(error.into())
        } else {
            Self::Subscribe(error)
        }
    }
}

#[async_trait]
impl<'proxy> SubscribeInterrupts for Toh<'proxy> {
    type Receiver = InterruptSubscription;
    type Error = SubscribeInterruptsError;

    async fn subscribe_interrupts(&mut self) -> Result<Self::Receiver, Self::Error> {
        let fd = self.toh.subscribe_interrupts().await?;
        let stream = StdUnixStream::from(OwnedFd::from(fd));
        stream.set_nonblocking(true)?;
        let stream = UnixStream::from_std(stream)?;
        Ok(InterruptSubscription { stream })
    }
}

/// Error while waiting for interrupts.
#[derive(Debug, Error)]
pub enum WaitInterruptError {
    /// TOH was disconnected.
    #[error("TOH disconnected")]
    Disconnected,
    /// IO error with the stream.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

#[async_trait]
impl WaitInterrupt for InterruptSubscription {
    type Error = WaitInterruptError;

    async fn wait_interrupt(&mut self) -> Result<Interrupt, Self::Error> {
        self.stream.readable().await?;
        let mut buf = [0u8; 64];
        loop {
            match self.stream.try_read(&mut buf) {
                Ok(0) => return Err(WaitInterruptError::Disconnected),
                Ok(_) => {
                    // Read the stream until there is no more data.
                    continue;
                }
                Err(ref error) if error.kind() == io::ErrorKind::ConnectionReset => {
                    return Err(WaitInterruptError::Disconnected)
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    // No more data, return interrupt.
                    return Ok(Interrupt);
                }
                Err(error) => {
                    return Err(error.into());
                }
            }
        }
    }
}
