// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! D-Bus server side stuff.

use super::error::*;
use crate::{
    i2cdev::I2cDev,
    power::Power,
    toh::{config::Permissions, Info},
};
use futures::StreamExt;
use std::collections::HashMap;
use std::io;
use std::os::fd::AsFd;
use std::sync::Arc;
use std::time::Duration;
use tokio::{
    sync::Mutex,
    time::sleep,
    {spawn, task::JoinHandle},
};
use zbus::{fdo::DBusProxy, interface, message::Header, names::UniqueName, Connection};
use zvariant::{Fd, Optional, OwnedValue};

// TODO: Similar borrowing interface for interrupts from TOH:
// I.e. the service should monitor interrupt pin and react differently to disconnects and TOH
// mcu initiated interrupts.

/// Representation of lent out i2c-dev access.
///
/// Note that we cannot possibly guarantee that the process borrowing access to the file descriptor
/// has exclusive access to it but we still try to do that for benefit of well-behaving clients.
/// The lending out mechanism also helps less privileged clients as they can be defined in TOH
/// configuration.
struct Loan {
    /// D-Bus bus name of the process that borrowed the file descriptor.
    owner: UniqueName<'static>,
    /// Power down after use.
    power_down: bool,
}

impl Loan {
    /// Creates loan and enables power until [`end`](Self::end) is called.
    async fn new(owner: UniqueName<'static>, power_down: bool) -> io::Result<Self> {
        Power::new().and_then(|mut pwr| pwr.set_power(true))?;
        sleep(Duration::from_millis(100)).await;
        Ok(Self { owner, power_down })
    }

    /// Ends the loan and turns off power.
    fn end(self) -> io::Result<()> {
        if self.power_down {
            Power::new().and_then(|mut pwr| pwr.set_power(false))
        } else {
            Ok(())
        }
    }
}

/// Join handle for loan watcher task and mutex for [`Loan`].
struct LoanWatcher {
    loan: Arc<Mutex<Option<Loan>>>,
    handle: JoinHandle<()>,
}

impl LoanWatcher {
    /// Create new loan with watcher for sender.
    async fn new(
        proxy: DBusProxy<'static>,
        sender: UniqueName<'static>,
        power_down: bool,
    ) -> io::Result<Self> {
        let loan = Arc::new(Mutex::new(Some(
            Loan::new(sender.clone(), power_down).await?,
        )));
        let arc = loan.clone();
        let handle = spawn(async move {
            let end_loan = move || async move {
                let mut guard = arc.lock().await;
                if let Some(loan) = guard.take() {
                    if let Err(error) = loan.end() {
                        log::warn!("Failed to turn off power after loan: {error}");
                    }
                }
            };
            if let Ok(mut stream) = proxy
                .receive_name_owner_changed_with_args(&[
                    (0, sender.as_str()),
                    (1, sender.as_str()),
                    (2, ""),
                ])
                .await
            {
                match proxy.name_has_owner(sender.clone().into()).await {
                    Err(error) => {
                        log::warn!("NameHasOwner check failed: {error}")
                    }
                    Ok(false) => {
                        // Loan ended early.
                        log::debug!("Owner not on bus, i2c-dev access returned.");
                        end_loan().await;
                    }
                    Ok(true) => {
                        while let Some(lost) = stream.next().await {
                            // Let's check again just in case.
                            if let Ok(args) = lost.args() {
                                if args.name() == &sender
                                    && args.old_owner().as_ref() == Some(&sender)
                                    && args.new_owner().is_none()
                                {
                                    log::debug!("Owner dropped from bus, i2c-dev access returned.");
                                    end_loan().await;
                                    break;
                                }
                            }
                            log::warn!("Wrong NameLost reported");
                        }
                    }
                }
            }
        });
        Ok(LoanWatcher { loan, handle })
    }

    /// Return current owner, if any.
    async fn owner(&self) -> Option<UniqueName<'static>> {
        self.loan
            .lock()
            .await
            .as_ref()
            .map(|loan| loan.owner.clone())
    }

    /// End loan and return result, if sender matches the borrower.
    async fn end_loan_on_match(&self, sender: &UniqueName<'_>) -> Option<io::Result<()>> {
        let mut guard = self.loan.lock().await;
        if let Some(loan) = guard.as_mut() {
            // Check that sender is the owner of the loan.
            if loan.owner == *sender {
                log::debug!("Sender matches loan owner, ending loan.");
                return guard.take().map(Loan::end);
            }
        }
        None
    }

    /// Abort watcher and clean up.
    async fn abort(self) {
        if !self.handle.is_finished() {
            self.handle.abort();
        }
        if let Err(error) = self.handle.await {
            if error.is_panic() {
                log::warn!("Loan watcher panicked");
            }
        }
    }

    /// Abort watcher and return loan without ending it first if there was any.
    async fn take_loan_and_abort(self) -> Option<Loan> {
        let loan = self.loan.lock().await.take();
        self.abort().await;
        loan
    }
}

/// Representation of TOH on D-Bus.
pub struct Toh {
    info: Info,
    permissions: Permissions,
    watcher: Option<LoanWatcher>,
}

impl Toh {
    /// Create new representation from TOH info.
    pub fn new(info: Info, permissions: Permissions) -> Self {
        Self {
            info,
            permissions,
            watcher: None,
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
        let sender = header.sender().ok_or(BorrowError::NoSender)?.to_owned();
        let proxy = DBusProxy::new(connection).await?;
        // First check if we have already lent the access.
        if let Some(watcher) = &self.watcher {
            if let Some(owner) = watcher.owner().await {
                if proxy.name_has_owner(owner.into()).await? {
                    // Already borrowed to a process and access has not been revoken.
                    return Err(BorrowError::AlreadyBorrowed);
                }
            }
        }
        // This needs to take care of the possible previous loan in case we are not lending again.
        let previous_loan = if let Some(watcher) = self.watcher.take() {
            // Technically we should not need to deal with this here as watcher should have always
            // taken care of this already. Allowing reborrow to the same process could change that
            // though.
            watcher.take_loan_and_abort().await
        } else {
            None
        };
        let creds = proxy
            .get_connection_credentials(sender.clone().into())
            .await?;
        // NB: We cannot use creds.unix_group_ids() because D-Bus does not populate it
        let ok = if creds.unix_user_id().ok_or(BorrowError::NoUid)? == 0 {
            // Root is always okay
            true
        } else {
            let pid: i32 = creds
                .process_id()
                .expect("Process ID is available on Linux")
                .try_into()
                .expect("Process ID fits into i32 as it is lower than 4194304");
            self.permissions.is_allowed_for_i2c_dev(pid)?
        };
        if ok {
            let fd = I2cDev::toh_dev()?.as_fd().try_clone_to_owned()?;
            log::debug!("Lending access to {sender}");
            self.watcher = Some(
                LoanWatcher::new(
                    proxy,
                    sender,
                    !leave_power_on.unwrap_or_else(|| self.info.leave_power_on.unwrap_or(false)),
                )
                .await?,
            );
            Ok(Fd::Owned(fd))
        } else {
            if let Some(loan) = previous_loan {
                if let Err(error) = loan.end() {
                    log::warn!("Failed to turn off power after loan: {error}");
                }
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
    /// This will only succeed if the caller is root or its executable is allowed in TOH
    /// configuration.
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
    async fn return_i2c_dev_access(
        &mut self,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), ReturnError> {
        let sender = header.sender().ok_or(ReturnError::NoSender)?;
        if let Some(watcher) = &self.watcher {
            if let Some(result) = watcher.end_loan_on_match(sender).await {
                self.watcher
                    .take()
                    .expect("There is a watcher")
                    .abort()
                    .await;
                return result.map_err(|e| e.into());
            }
        }
        Err(ReturnError::NoLoan)
    }
}
