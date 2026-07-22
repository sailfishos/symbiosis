// Copyright (c) 2026 Jolla Mobile Ltd

//! D-Bus server side stuff.

use super::error::*;
use crate::{i2cdev::I2cDev, toh::Info};
use std::collections::HashMap;
use std::os::fd::AsFd;
use zbus::{fdo::DBusProxy, interface, message::Header, names::UniqueName, Connection};
use zvariant::{Fd, Optional, OwnedValue};

// TODO: Similar borrowing interface for interrupts from TOH:
// I.e. the service should monitor interrupt pin and react differently to disconnects and TOH
// mcu initiated interrupts.

/// Representation of lent out i2c-dev access.
///
/// Note that we cannot possibly guarantee that the process borrowing access to the file descriptor
/// has exclusive access to it but we still try to do that for benefit of well-behaving clients.
/// The lending out mechanism also helps less privileged clients once the policies around that have
/// been decided and implemented.
///
/// In the future we may hand out these loans to more specific processes, such as those that are
/// specified in some TOH specific configuration files, so rejection might happen on other reasons
/// than caller having the wrong user.
struct Loan {
    /// D-Bus bus name of the process that borrowed the file descriptor.
    owner: UniqueName<'static>,
    // TODO: This could keep a pidfd for the process too.
}

/// Representation of TOH on D-Bus.
pub struct Toh {
    info: Info,
    loan: Option<Loan>,
}

impl Toh {
    /// Create new representation from TOH info.
    pub fn new(info: Info) -> Self {
        Self { info, loan: None }
    }
}

#[interface(name = "org.sailfishos.tohd1.Toh")]
impl Toh {
    #[zbus(property)]
    fn vendor_id(&self) -> u16 {
        self.info.vendor_id
    }

    #[zbus(property)]
    fn product_id(&self) -> u16 {
        self.info.product_id
    }

    #[zbus(property)]
    fn schema_version(&self) -> Optional<u8> {
        self.info.schema_version.into()
    }

    #[zbus(property)]
    fn serial_number(&self) -> Optional<String> {
        self.info.serial_number.clone().into()
    }

    #[zbus(property)]
    fn vendor_name(&self) -> Optional<String> {
        self.info.vendor_name.clone().into()
    }

    #[zbus(property)]
    fn product_name(&self) -> Optional<String> {
        self.info.product_name.clone().into()
    }

    #[zbus(property)]
    fn vendor_website(&self) -> Optional<String> {
        self.info.vendor_website.clone().into()
    }

    #[zbus(property)]
    fn product_website(&self) -> Optional<String> {
        self.info.product_website.clone().into()
    }

    #[zbus(property)]
    fn leave_power_on(&self) -> Optional<bool> {
        self.info.leave_power_on.into()
    }

    #[zbus(property)]
    fn power_input_toh(&self) -> Optional<bool> {
        self.info.power_input_toh.into()
    }

    #[zbus(property)]
    fn extra_data(&self) -> HashMap<String, OwnedValue> {
        self.info
            .extra
            .iter()
            .filter_map(|(k, v)| Some((k, v.try_into_dbus_lossy().ok()?)))
            .map(|(k, v)| {
                (
                    k.clone(),
                    v.try_to_owned().expect("None of the values is an fd here"),
                )
            })
            .collect::<HashMap<_, _>>()
    }

    // NB: Semantics of these methods will be refined when the implementation progresses.

    /// Borrow i2c-dev access to the I²C bus.
    ///
    /// Currently only available for processes running as root.
    async fn borrow_i2c_dev_access(
        &mut self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> Result<Fd<'static>, BorrowError> {
        let sender = header.sender().ok_or(BorrowError::NoSender)?.to_owned();
        let proxy = DBusProxy::new(connection).await?;
        // TODO: Check if caller has sufficient access rights
        if let Some(loan) = &self.loan {
            if proxy.name_has_owner(loan.owner.clone().into()).await? {
                return Err(BorrowError::AlreadyBorrowed);
            } else {
                // Loan has expired, we can lend it again.
                self.loan = None
            }
        }
        let uid = proxy
            .get_connection_unix_user(sender.clone().into())
            .await?;
        if uid == 0 {
            // Currently only root can get access to the file descriptor.
            // This may be later extended to allow for some other conditions too.
            let fd = I2cDev::toh_dev()?.as_fd().try_clone_to_owned()?;
            self.loan = Some(Loan { owner: sender });
            Ok(Fd::Owned(fd))
        } else {
            Err(BorrowError::AccessDenied)
        }
    }

    /// Return i2c-dev access to the I²C bus.
    ///
    /// Only available to the process that had borrowed it.
    fn return_i2c_dev_access(
        &mut self,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), ReturnError> {
        if let Some(loan) = &self.loan {
            if let Some(sender) = header.sender() {
                if loan.owner == *sender {
                    self.loan = None;
                    return Ok(());
                }
            } else {
                return Err(ReturnError::NoSender);
            }
        }
        Err(ReturnError::NoLoan)
    }
}
