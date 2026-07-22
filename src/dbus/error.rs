// Copyright (c) 2026 Jolla Mobile Ltd

//! D-Bus errors.

use zbus::DBusError;

#[derive(DBusError, Debug)]
#[zbus(prefix = "org.sailfishos.tohd1")]
pub enum BorrowError {
    /// The process trying to borrow is not run as root.
    ///
    /// This restriction may get lifted in the future if suitable access control mechanism is found.
    AccessDenied,
    /// The device was already borrowed.
    AlreadyBorrowed,
    /// No sender in message header.
    NoSender,
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

#[derive(DBusError, Debug)]
#[zbus(prefix = "org.sailfishos.tohd1")]
pub enum ReturnError {
    /// There was no active loan for the caller.
    NoLoan,
    /// No sender in message header.
    NoSender,
}
