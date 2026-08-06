// Copyright (c) 2026 Jolla Mobile Ltd

//! This library crate contains parts of TOH daemon.
//!
//! The daemon implements detecting presence, fetching and publishing info, and starting and stopping
//! services. All of this is done in TOH agnostic way.
//!
//! If you are implementing services for TOH, you'll likely want to use the D-Bus interface and not
//! include this code.

pub mod back_cover;
pub mod dbus;
pub mod errors;
pub mod i2cdev;
pub mod id;
pub mod interrupt;
pub mod power;
mod systemd;
pub mod toh;
