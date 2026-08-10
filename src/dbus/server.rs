// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! D-Bus server side stuff.

use super::error::*;
use crate::{i2cdev::I2cDev, power::Power, toh::Info};
use log::warn;
use nix::unistd::Group;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::os::fd::AsFd;
use std::time::Duration;
use tokio::time::sleep;
use zbus::{fdo::DBusProxy, interface, message::Header, names::UniqueName, Connection};
use zvariant::{Fd, Optional, OwnedValue};

// TODO: Similar borrowing interface for interrupts from TOH:
// I.e. the service should monitor interrupt pin and react differently to disconnects and TOH
// mcu initiated interrupts.

// TODO: Add watching for the client that has borrowed i2c-dev so that we power TOH down if the
// client leaves the bus.

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
    /// Power down after use.
    power_down: bool,
}

/// Representation of TOH on D-Bus.
pub struct Toh {
    info: Info,
    loan: Option<Loan>,
    privileged_group: Option<NonZeroU32>,
}

impl Toh {
    /// Create new representation from TOH info.
    pub fn new(info: Info) -> Self {
        let privileged_group = Group::from_name("privileged")
            .ok()
            .flatten()
            .and_then(|group| NonZeroU32::new(group.gid.as_raw()));
        if privileged_group.is_none() {
            warn!("Privileged gid could not be fetched, methods are not available for privileged group");
        }
        Self {
            info,
            loan: None,
            privileged_group,
        }
    }
}

impl Toh {
    async fn lend_i2c_dev(
        &mut self,
        leave_power_on: Option<bool>,
        header: Header<'_>,
        connection: &Connection,
    ) -> Result<Fd<'static>, BorrowError> {
        // TODO: Maybe we could do powering down conditionally a bit smarter with some guard type.
        let mut power_down = false;
        let sender = header.sender().ok_or(BorrowError::NoSender)?.to_owned();
        let proxy = DBusProxy::new(connection).await?;
        if let Some(loan) = &self.loan {
            if proxy.name_has_owner(loan.owner.clone().into()).await? {
                return Err(BorrowError::AlreadyBorrowed);
            } else {
                // Loan has expired, we can lend it again.
                power_down = loan.power_down;
                self.loan = None
            }
        }
        let creds = proxy
            .get_connection_credentials(sender.clone().into())
            .await?;
        // NB: We cannot use creds.unix_group_ids() because D-Bus does not populate it
        let ok = if creds.unix_user_id().ok_or(BorrowError::NoUid)? == 0 {
            // Root is always okay
            true
        } else if let Some(privileged_group) = self.privileged_group {
            // Privileged group is something special on Sailfish OS
            let pid: i32 = creds
                .process_id()
                .expect("Process ID is available on Linux")
                .try_into()
                .expect("Process ID fits into i32 as it is lower than 4194304");
            let status = procfs::process::Process::new(pid)?.status()?;
            status.egid == privileged_group.into()
                || status.groups.contains(&privileged_group.into())
        } else {
            // Currently no-one else can access this
            false
        };
        if ok {
            // Enable power for the duration of the loan.
            Power::new().and_then(|mut pwr| pwr.set_power(true))?;
            sleep(Duration::from_millis(100)).await;
            let fd = I2cDev::toh_dev()?.as_fd().try_clone_to_owned()?;
            self.loan = Some(Loan {
                owner: sender,
                power_down: !leave_power_on
                    .unwrap_or_else(|| self.info.leave_power_on.unwrap_or(false)),
            });
            Ok(Fd::Owned(fd))
        } else {
            if power_down {
                Power::new().and_then(|mut pwr| pwr.set_power(false))?;
            }
            Err(BorrowError::AccessDenied)
        }
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
    /// Also powers up the TOH if it was powered down.
    ///
    /// Currently only available for processes running as root or as privileged group.
    async fn borrow_i2c_dev_access(
        &mut self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> Result<Fd<'static>, BorrowError> {
        self.lend_i2c_dev(None, header, connection).await
    }

    /// Borrow i2c-dev access to the I²C bus.
    ///
    /// Set `leave_power_on` to `true` if you want the power to stay on after returning the access.
    ///
    /// See also [`borrow_i2c_dev_access`](Self::borrow_i2c_dev_access).
    async fn borrow_i2c_dev_access_with_power(
        &mut self,
        leave_power_on: bool,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> Result<Fd<'static>, BorrowError> {
        self.lend_i2c_dev(Some(leave_power_on), header, connection)
            .await
    }

    /// Return i2c-dev access to the I²C bus.
    ///
    /// Also powers down the TOH.
    ///
    /// Only available to the process that had borrowed the access.
    fn return_i2c_dev_access(
        &mut self,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), ReturnError> {
        if let Some(loan) = &self.loan {
            if let Some(sender) = header.sender() {
                if loan.owner == *sender {
                    let result = if loan.power_down {
                        Power::new().and_then(|mut pwr| pwr.set_power(false))
                    } else {
                        Ok(())
                    };
                    self.loan = None;
                    return result.map_err(|e| e.into());
                }
            } else {
                return Err(ReturnError::NoSender);
            }
        }
        Err(ReturnError::NoLoan)
    }
}
