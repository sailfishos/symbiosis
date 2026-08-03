// Copyright (c) 2026 Jolla Mobile Ltd

//! All things config.

mod configs;
pub(crate) mod parse;

pub use configs::{ConfigError, Configs, Overrides, Units};
