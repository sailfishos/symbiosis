// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! All generic TOH related things.

mod client;
pub(crate) mod config;
mod info;
pub mod traits;
mod value;

pub use crate::dbus::error::{BorrowError, ReturnError, SubscribeError};
pub use client::{
    FetchingInfoError, InterruptSubscription, SubscribeInterruptError, Toh, WaitInterruptError,
};
pub use config::{ConfigError, Configs, Devices, Overrides, Units};
pub use info::*;
pub use traits::*;
pub use value::{ExtraValue, NonConvertableValue};
