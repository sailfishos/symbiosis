// Copyright (c) 2026 Jolla Mobile Ltd

//! Access to TOH through org.sailfishos.tohd1.

use super::*;
use crate::{
    dbus::error::{BorrowError, ReturnError},
    dbus::TohProxy,
    errors::ExtraValueConversionError,
    i2cdev::I2cDev,
    power::Power,
};
use async_trait::async_trait;
use futures::future::BoxFuture;
use std::collections::BTreeMap;
use thiserror::Error;
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
        let mut dev = I2cDev::from_fd(fd.into());
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
        let mut dev = I2cDev::from_fd(fd.into());
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
    type Error = std::io::Error;
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
