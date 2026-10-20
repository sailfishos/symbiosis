// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! I2cDev loan management.

use crate::power::Power;
use futures::StreamExt;
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::{
    sync::Mutex,
    time::sleep,
    {spawn, task::JoinHandle},
};
use zbus::{fdo::DBusProxy, names::UniqueName};

/// Representation of lent out i2c-dev access.
///
/// Note that we cannot possibly guarantee that the process borrowing access to the file descriptor
/// has exclusive access to it but we still try to do that for benefit of well-behaving clients.
/// The lending out mechanism also helps less privileged clients as they can be defined in TOH
/// configuration.
pub(crate) struct Loan {
    /// D-Bus bus name of the process that borrowed the file descriptor.
    owner: UniqueName<'static>,
    // TODO: This could keep a pidfd for the process too so we can check if it is still holding the
    // device open.
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
    pub fn end(self) -> io::Result<()> {
        if self.power_down {
            Power::new().and_then(|mut pwr| pwr.set_power(false))
        } else {
            Ok(())
        }
    }
}

/// Join handle for loan watcher task and mutex for [`Loan`].
pub(crate) struct LoanWatcher {
    loan: Arc<Mutex<Option<Loan>>>,
    handle: JoinHandle<()>,
}

impl LoanWatcher {
    /// Create new loan with watcher for sender.
    pub async fn new(
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
    pub async fn owner(&self) -> Option<UniqueName<'static>> {
        self.loan
            .lock()
            .await
            .as_ref()
            .map(|loan| loan.owner.clone())
    }

    /// End loan and return result, if sender matches the borrower.
    pub async fn end_loan_on_match(&self, sender: &UniqueName<'_>) -> Option<io::Result<()>> {
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
    pub async fn abort(self) {
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
    pub async fn take_loan_and_abort(self) -> Option<Loan> {
        let loan = self.loan.lock().await.take();
        self.abort().await;
        loan
    }
}
