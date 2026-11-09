// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! D-Bus errors.

use zbus::DBusError;

/// Error while borrowing access to i2c-dev.
#[derive(DBusError, Debug)]
#[zbus(prefix = "org.sailfishos.tohd1")]
pub enum BorrowError {
    /// The process trying to borrow is not allowed to borrow by configuration.
    ///
    /// When caller is _root_ it is allowed to borrow it regardless of the configuration.
    AccessDenied,
    /// The device was already borrowed.
    AlreadyBorrowed,
    /// No sender in message header.
    NoSender,
    /// No uid in credentials returned by D-Bus.
    NoUid,
    /// No process status or process status could not be fetched.
    NoProcessStatus,
    /// IO error.
    IOError,
    /// Internal errors or anything else.
    #[zbus(error)]
    ZBus(zbus::Error),
}

impl From<zbus::fdo::Error> for BorrowError {
    fn from(error: zbus::fdo::Error) -> Self {
        BorrowError::ZBus(error.into())
    }
}

impl From<std::io::Error> for BorrowError {
    fn from(_error: std::io::Error) -> Self {
        BorrowError::IOError
    }
}

impl From<procfs::ProcError> for BorrowError {
    fn from(_error: procfs::ProcError) -> Self {
        BorrowError::NoProcessStatus
    }
}

/// Error while returning access to i2c-dev.
#[derive(DBusError, Debug)]
#[zbus(prefix = "org.sailfishos.tohd1")]
pub enum ReturnError {
    /// There was no active loan for the caller.
    NoLoan,
    /// No sender in message header.
    NoSender,
    /// Failed to power down.
    PowerDownFailed,
    /// Internal errors or anything else.
    #[zbus(error)]
    ZBus(zbus::Error),
}

impl From<std::io::Error> for ReturnError {
    fn from(_error: std::io::Error) -> Self {
        ReturnError::PowerDownFailed
    }
}

/// Error while subscribing to interrupts.
#[derive(DBusError, Debug)]
#[zbus(prefix = "org.sailfishos.tohd1")]
pub enum SubscribeError {
    /// The process trying to subscribe but it is not allowed by configuration.
    ///
    /// When caller is _root_ it is allowed to subscribe regardless of the configuration.
    AccessDenied,
    /// No sender in message header.
    NoSender,
    /// No uid in credentials returned by D-Bus.
    NoUid,
    /// No process status or process status could not be fetched.
    NoProcessStatus,
    /// IO error.
    IOError,
    /// Internal errors or anything else.
    #[zbus(error)]
    ZBus(zbus::Error),
}

impl From<zbus::fdo::Error> for SubscribeError {
    fn from(error: zbus::fdo::Error) -> Self {
        SubscribeError::ZBus(error.into())
    }
}

impl From<std::io::Error> for SubscribeError {
    fn from(_error: std::io::Error) -> Self {
        SubscribeError::IOError
    }
}

impl From<procfs::ProcError> for SubscribeError {
    fn from(_error: procfs::ProcError) -> Self {
        SubscribeError::NoProcessStatus
    }
}
