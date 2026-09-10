// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! TOH configs.
//!
//! These are yaml files that are on the device, they can override values provided by the memory
//! chip and extend TOH functionality by executing services on TOH connect.

use super::parse::{self, combine};
use crate::systemd::Manager;
use crate::toh::Info;
use std::collections::HashSet;
use std::fs::read_dir;
use std::marker::{PhantomData, Send};
use std::path::PathBuf;
use thiserror::Error;
use yaml_serde::Error as YamlError;

mod paths {
    pub const CONFIG_PATH: &str = "/usr/share/tohd-1/tohs/";
}

/// Reading configs failed.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// IO error while looking for config files for TOH.
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    /// IO error while reading a config file.
    #[error("IO error in '{0}': {1}")]
    IOErrorWithFile(PathBuf, std::io::Error),
    /// Parsing error.
    #[error("YAML parsing error in '{0}': {1}")]
    ParsingError(PathBuf, YamlError),
}

/// Configs for TOH.
#[derive(Debug, Clone, Default)]
pub struct Configs {
    overrides: parse::Override,
    system_units: Vec<parse::SystemdUnit>,
    user_units: Vec<parse::SystemdUnit>,
    access: parse::Access,
    devices: Vec<parse::Device>,
}

/// Overrides from [`Configs`].
///
/// These are applied to [`Info`] instances.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    overrides: parse::Override,
}

mod state {
    /// Assumed unit state.
    pub trait State {}

    /// Services have been stopped.
    pub struct Stopped;

    /// Services have been started.
    pub struct Started;

    impl State for Stopped {}
    impl State for Started {}
}

/// Systemd units from [`Configs`].
///
/// System and user units to start for TOH.
#[derive(Debug, Default)]
pub struct Units<S: state::State + Send> {
    system_units: Vec<parse::SystemdUnit>,
    user_units: Vec<parse::SystemdUnit>,
    _state: PhantomData<S>,
}

/// Access permissions from [`Configs`].
#[derive(Debug, Default)]
pub struct Permissions {
    /// Binaries that can access i2c-dev
    i2c_dev_exe_paths: HashSet<PathBuf>,
}

/// Bound target devices on bus from [`Configs`].
#[derive(Debug, Default)]
pub struct Devices(pub(crate) Vec<parse::Device>);

impl Configs {
    pub(crate) fn find(vendor_id: u16, product_id: u16) -> Result<Option<Self>, ConfigError> {
        // Config files are in paths::CONFIG_PATH/vendor_id/product_id directory
        let mut path = PathBuf::from(paths::CONFIG_PATH);
        path.push(format!("{:04x}", vendor_id));
        path.push(format!("{:04x}", product_id));
        if !std::fs::metadata(&path)
            .as_ref()
            .is_ok_and(std::fs::Metadata::is_dir)
        {
            return Ok(None);
        }
        // TODO: The directory could have been deleted between the check and this
        let mut paths: Vec<PathBuf> = read_dir(&path)?
            .filter_map(|entry| match entry {
                Ok(entry) => {
                    let path = entry.path();
                    if path
                        .extension()
                        .and_then(std::ffi::OsStr::to_str)
                        .map(|ex| ex == "yaml")
                        .unwrap_or(false)
                    {
                        Some(Ok(path))
                    } else {
                        None
                    }
                }
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<_, _>>()?;
        if paths.is_empty() {
            Ok(None)
        } else {
            // Deterministic order and also allows xx-name.yaml notation if needed
            paths.sort();

            let mut configs = Configs::default();
            paths
                .into_iter()
                .map(|path| {
                    parse::Config::from_path(&path).map_err(|error| {
                        use parse::ParsingError::*;
                        match error {
                            IOError(error) => ConfigError::IOErrorWithFile(path.clone(), error),
                            ParsingError(error) => ConfigError::ParsingError(path.clone(), error),
                        }
                    })
                })
                .try_for_each(|config| config.map(|config| configs.update(config)))?;
            Ok(Some(configs))
        }
    }

    fn update(&mut self, config: parse::Config) {
        let parse::Config {
            overrides,
            system_unit,
            user_unit,
            access,
            devices,
        } = config;
        if let Some(overrides) = overrides {
            self.overrides.with_other(overrides);
        }
        if let Some(system_unit) = system_unit {
            self.system_units.push(system_unit);
        }
        if let Some(user_unit) = user_unit {
            self.user_units.push(user_unit);
        }
        if let Some(access) = access {
            self.access.i2c_dev.extend(access.i2c_dev);
        }
        self.devices.extend(devices);
    }

    /// Splits the config into overrides and unit configurations.
    pub fn split(self) -> (Overrides, Devices, Units<state::Stopped>, Permissions) {
        let Self {
            overrides,
            devices,
            system_units,
            user_units,
            access,
        } = self;
        (
            Overrides { overrides },
            Devices(devices),
            Units {
                system_units,
                user_units,
                _state: PhantomData,
            },
            Permissions {
                i2c_dev_exe_paths: access
                    .i2c_dev
                    .into_iter()
                    .map(|value| value.into())
                    .collect(),
            },
        )
    }
}

impl Overrides {
    pub(crate) fn apply_overrides(self, info: &mut Info) {
        // If we had even more of these, it would probably make sense to write a derive macro.
        combine!(info, self.overrides, vendor_name);
        combine!(info, self.overrides, product_name);
        combine!(info, self.overrides, vendor_website);
        combine!(info, self.overrides, product_website);
        combine!(info, self.overrides, leave_power_on);
        combine!(info, self.overrides, power_input_toh);
        info.extra.extend(self.overrides.extra);
    }
}

impl Units<state::Stopped> {
    /// Starts units found in configuration.
    pub async fn start_units(self, on_service_start: bool) -> Units<state::Started> {
        let Self {
            system_units,
            user_units,
            ..
        } = self;
        if !system_units.is_empty() {
            match Manager::system().await {
                Ok(mut manager) => {
                    for unit in system_units
                        .iter()
                        .filter(|unit| !on_service_start || unit.run_on_start)
                    {
                        log::debug!("Starting {} in system session", unit.name);
                        if let Err(error) = manager.start_unit(unit).await {
                            log::warn!("Failed to start unit {}: {error}", unit.name);
                        }
                    }
                }
                Err(error) => {
                    log::error!("Failed to access system manager: {error}");
                }
            }
        }
        if !user_units.is_empty() {
            match Manager::session().await {
                Ok(mut manager) => {
                    for unit in user_units
                        .iter()
                        .filter(|unit| !on_service_start || unit.run_on_start)
                    {
                        log::debug!("Starting {} in user session", unit.name);
                        if let Err(error) = manager.start_unit(unit).await {
                            log::warn!("Failed to start unit {}: {error}", unit.name);
                        }
                    }
                }
                Err(error) => {
                    log::error!("Failed to access user manager: {error}");
                }
            }
        }
        Units {
            system_units,
            user_units,
            _state: PhantomData,
        }
    }
}

impl Units<state::Started> {
    /// Stops started units.
    pub async fn stop_units(self) -> Units<state::Stopped> {
        let Self {
            system_units,
            user_units,
            ..
        } = self;
        if !system_units.is_empty() {
            match Manager::system().await {
                Ok(mut manager) => {
                    for unit in &system_units {
                        log::debug!("Stopping {} in system session", unit.name);
                        if let Err(error) = manager.stop_unit(unit).await {
                            log::warn!("Failed to stop unit {}: {error}", unit.name);
                        }
                    }
                }
                Err(error) => {
                    log::error!("Failed to access system manager: {error}");
                }
            }
        }
        if !user_units.is_empty() {
            match Manager::session().await {
                Ok(mut manager) => {
                    for unit in &user_units {
                        log::debug!("Stopping {} in user session", unit.name);
                        if let Err(error) = manager.stop_unit(unit).await {
                            log::warn!("Failed to stop unit {}: {error}", unit.name);
                        }
                    }
                }
                Err(error) => {
                    log::error!("Failed to access user manager: {error}");
                }
            }
        }
        Units {
            system_units,
            user_units,
            _state: PhantomData,
        }
    }
}

impl Permissions {
    /// Checks if process is allowed to access i2c-dev device.
    pub(crate) fn is_allowed_for_i2c_dev(&self, pid: i32) -> Result<bool, procfs::ProcError> {
        let path = procfs::process::Process::new(pid)?.exe()?;
        Ok(!path.ends_with(" (deleted)") && self.i2c_dev_exe_paths.contains(&path))
    }
}
