// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! All things config.

mod configs;
pub(crate) mod parse;

pub use configs::{ConfigError, Configs, Overrides, Permissions, Units};
