// Copyright (c) 2026 Jolla Mobile Ltd

//! D-Bus proxy for the interface.

use super::error::*;
use std::collections::HashMap;
use zbus::{fdo, proxy};
use zvariant::{Optional, OwnedFd, OwnedValue};

/// Proxy trait for org.sailfishos.tohd1.Toh interface.
///
/// This the client side counterpart for crate::dbus::server::Toh.
#[proxy(
    interface = "org.sailfishos.tohd1.Toh",
    default_service = "org.sailfishos.tohd1",
    default_path = "/org/sailfishos/tohd1/toh"
)]
pub trait Toh {
    #[zbus(property)]
    fn vendor_id(&self) -> fdo::Result<u16>;

    #[zbus(property)]
    fn product_id(&self) -> fdo::Result<u16>;

    #[zbus(property)]
    fn schema_version(&self) -> fdo::Result<Optional<u8>>;

    #[zbus(property)]
    fn serial_number(&self) -> fdo::Result<Optional<String>>;

    #[zbus(property)]
    fn vendor_name(&self) -> fdo::Result<Optional<String>>;

    #[zbus(property)]
    fn product_name(&self) -> fdo::Result<Optional<String>>;

    #[zbus(property)]
    fn vendor_website(&self) -> fdo::Result<Optional<String>>;

    #[zbus(property)]
    fn product_website(&self) -> fdo::Result<Optional<String>>;

    #[zbus(property)]
    fn leave_power_on(&self) -> fdo::Result<Optional<bool>>;

    #[zbus(property)]
    fn power_input_toh(&self) -> fdo::Result<Optional<bool>>;

    #[zbus(property)]
    fn extra_data(&self) -> fdo::Result<HashMap<String, OwnedValue>>;

    /// Borrow i2c-dev access to the I²C bus.
    ///
    /// Currently only available for processes running as root.
    async fn borrow_i2c_dev_access(&mut self) -> Result<OwnedFd, BorrowError>;

    /// Return i2c-dev access to the I²C bus.
    ///
    /// Only available to the process that had borrowed it.
    fn return_i2c_dev_access(&mut self) -> Result<(), ReturnError>;
}
