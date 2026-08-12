// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! Types for parsing config files.

use crate::toh::ExtraValue;
use derive_more::Into;
use serde::{de::Error, de::SeqAccess, de::Visitor, Deserialize, Deserializer};
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use thiserror::Error;
use yaml_serde::{Error as YamlError, Value};

/// Add field in b to a if the field in b is Some.
macro_rules! combine {
    ($a:expr, $b:expr, $field:ident) => {
        $a.$field = $b.$field.or_else(|| $a.$field.take())
    };
}
pub(crate) use combine;

/// Config parsing error.
#[derive(Debug, Error)]
pub(crate) enum ParsingError {
    /// IO error.
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    /// Parsing error.
    #[error("YAML parsing error: {0}")]
    ParsingError(#[from] YamlError),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct Config {
    #[serde(rename = "override")]
    pub overrides: Option<Override>,
    pub system_unit: Option<SystemdUnit>,
    pub user_unit: Option<SystemdUnit>,
    pub access: Option<Access>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct Override {
    pub vendor_name: Option<String>,
    pub product_name: Option<String>,
    pub vendor_website: Option<String>,
    pub product_website: Option<String>,
    pub leave_power_on: Option<bool>,
    pub power_input_toh: Option<bool>,
    #[serde(flatten, deserialize_with = "deserialize_map_with_extra_value")]
    pub extra: HashMap<String, ExtraValue>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct SystemdUnit {
    #[serde(rename = "service-name")]
    pub name: String,
    #[serde(default = "yes")]
    pub run_on_start: bool,
    #[serde(flatten, rename = "type")]
    pub unit_type: Unit,
}

// TODO: What other types could we need?
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(crate) enum Unit {
    TransientService(TransientService),
    Service,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct TransientService {
    pub description: String,
    #[serde(default)]
    pub service_type: ServiceType,
    #[serde(rename = "service-exec")]
    pub exec: Exec,
    #[serde(rename = "service-exec-stop")]
    pub exec_stop: Option<Exec>,
}

// TODO: Support the other types too: DBus, Forking, Notify, NotifyReload
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, strum::Display)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ServiceType {
    #[strum(to_string = "simple")]
    #[default]
    Simple,
    #[strum(to_string = "exec")]
    Exec,
    #[strum(to_string = "oneshot")]
    Oneshot,
}

/// Newtype that guarantees the path is absolute and has a file name.
#[derive(Debug, Clone, PartialEq, Deserialize, Into)]
pub(crate) struct Executable(PathBuf);

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Exec {
    bin: Executable,
    args: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct Access {
    #[serde(default)]
    pub i2c_dev: Vec<Executable>,
}

impl Config {
    pub(crate) fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, ParsingError> {
        let file = File::open(&path)?;
        let config = yaml_serde::from_reader(file)?;
        Ok(config)
    }
}

fn deserialize_map_with_extra_value<'de, D>(de: D) -> Result<HashMap<String, ExtraValue>, D::Error>
where
    D: Deserializer<'de>,
{
    HashMap::<String, Value>::deserialize(de)?
        .into_iter()
        .map(|(key, value)| {
            Ok((
                key.clone(),
                ExtraValue::try_from(value).map_err(|error| {
                    D::Error::custom(format!(
                        "Could not convert value of {key} into ExtraValue: {error}"
                    ))
                })?,
            ))
        })
        .collect::<Result<HashMap<String, ExtraValue>, D::Error>>()
}

impl Override {
    pub(crate) fn with_other(&mut self, other: Self) {
        // If we had even more of these, it would probably make sense to write a derive macro.
        combine!(self, other, vendor_name);
        combine!(self, other, product_name);
        combine!(self, other, vendor_website);
        combine!(self, other, product_website);
        combine!(self, other, leave_power_on);
        combine!(self, other, power_input_toh);
        self.extra.extend(other.extra);
    }
}

struct ExecVisitor;

impl<'de> Visitor<'de> for ExecVisitor {
    type Value = Exec;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a path to an executable and arguments as a sequence")
    }

    fn visit_seq<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        if let Some(bin) = access.next_element::<String>()? {
            // Converting this to ensure that it converts back to String later.
            match Executable::try_from(bin) {
                Err(InvalidExecutable::NotAbsolutePath) => {
                    Err(A::Error::missing_field("executable path is not absolute"))
                }
                Err(InvalidExecutable::NoFileName) => Err(A::Error::missing_field(
                    "executable path file name is missing",
                )),
                Ok(bin) => {
                    let mut args = if let Some(capacity) = access.size_hint() {
                        Vec::with_capacity(capacity)
                    } else {
                        Vec::new()
                    };
                    while let Some(arg) = access.next_element::<String>()? {
                        args.push(arg);
                    }
                    Ok(Self::Value { bin, args })
                }
            }
        } else {
            Err(A::Error::missing_field("expected a path to an executable"))
        }
    }
}

impl<'de> Deserialize<'de> for Exec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(ExecVisitor)
    }
}

/// Bad executable path provided.
///
/// Returned when the first value does not have the right format.
#[derive(Debug, Error)]
pub(crate) enum InvalidExecutable {
    /// Missing executable file name.
    ///
    /// Path did not contain file name.
    #[error("Missing executable name")]
    NoFileName,
    /// Executable path must be an absolute path.
    #[error("Executable path is not absolute")]
    NotAbsolutePath,
}

impl TryFrom<PathBuf> for Executable {
    type Error = InvalidExecutable;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        if !path.is_absolute() {
            Err(InvalidExecutable::NotAbsolutePath)
        } else if path.file_name().is_none() {
            Err(InvalidExecutable::NoFileName)
        } else {
            Ok(Self(path))
        }
    }
}

impl TryFrom<&str> for Executable {
    type Error = InvalidExecutable;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let path: PathBuf = value.into();
        path.try_into()
    }
}

impl TryFrom<String> for Executable {
    type Error = InvalidExecutable;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let path: PathBuf = value.into();
        path.try_into()
    }
}

/// Bad executable path provided.
///
/// Returned when the first value does not have the right format.
#[derive(Debug, Error)]
pub(crate) enum BadExecutable {
    /// Missing executable path.
    ///
    /// There must be at least an absolute path to the executable.
    #[error("Missing executable path")]
    Missing,
    /// Invalid executable.
    #[error("{0}")]
    Invalid(#[from] InvalidExecutable),
}

impl TryFrom<Vec<&str>> for Exec {
    type Error = BadExecutable;

    fn try_from(vec: Vec<&str>) -> Result<Self, Self::Error> {
        let mut it = vec.into_iter();
        if let Some(bin) = it.next() {
            Ok(Self {
                bin: bin.try_into()?,
                args: Vec::from_iter(it.map(ToOwned::to_owned)),
            })
        } else {
            Err(BadExecutable::Missing)
        }
    }
}

impl Exec {
    pub(crate) fn path(&self) -> &str {
        self.bin
            .0
            .to_str()
            .expect("Validity as string was checked during parsing")
    }

    pub(crate) fn args(&self) -> Vec<&str> {
        let (_, bin) = self
            .bin
            .0
            .to_str()
            .expect("Validity as string was checked during parsing")
            .rsplit_once('/')
            .expect("Binary path contains at least one '/'");
        std::iter::once(bin)
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_overrides() {
        let config: Config = yaml_serde::from_str(
            "
--- # Testing overrides here
override:
  vendor-name: testy
  product-name: tester
  vendor-website: https://example.com/
  leave-power-on: true
  power-input-toh: false
  some-text: testing out
  a-boolean: false
",
        )
        .unwrap();
        let overrides = config.overrides.unwrap();
        assert_eq!(overrides.vendor_name, Some("testy".to_string()));
        assert_eq!(overrides.product_name, Some("tester".to_string()));
        assert_eq!(
            overrides.vendor_website,
            Some("https://example.com/".to_string())
        );
        assert_eq!(overrides.product_website, None);
        assert_eq!(overrides.leave_power_on, Some(true));
        assert_eq!(overrides.power_input_toh, Some(false));
        assert_eq!(
            overrides.extra.get("some-text"),
            Some(&"testing out".to_string().into())
        );
        assert_eq!(overrides.extra.get("a-boolean"), Some(&false.into()));
        assert!(config.system_unit.is_none());
        assert!(config.user_unit.is_none());
        assert!(config.access.is_none());
    }

    #[test]
    fn parse_unit() {
        let config: Config = yaml_serde::from_str(
            "
--- # Just some transient units to check
user-unit:
  description: My Test Unit
  type: transient-service
  service-type: oneshot
  service-name: my-test-unit
  service-exec:
    - \"/path/to/binary\"
    - \"arg1\"
    - \"arg2\"

system-unit:
  description: Such a failure
  type: transient-service
  service-name: another-one
  service-exec: [\"/usr/bin/false\"]
  service-exec-stop: [\"/usr/bin/true\"]
  run-on-start: false
",
        )
        .unwrap();
        assert!(config.overrides.is_none());
        assert!(config.access.is_none());
        let unit = config.user_unit.unwrap();
        assert_eq!(unit.name, "my-test-unit");
        assert!(unit.run_on_start);
        assert!(matches!(unit.unit_type, Unit::TransientService(_)));
        let Unit::TransientService(service) = unit.unit_type else {
            panic!("Impossible")
        };
        assert_eq!(service.description, "My Test Unit");
        assert_eq!(service.service_type, ServiceType::Oneshot);
        assert_eq!(
            service.exec,
            vec!["/path/to/binary", "arg1", "arg2"].try_into().unwrap()
        );
        assert!(service.exec_stop.is_none());
        let unit = config.system_unit.unwrap();
        assert_eq!(unit.name, "another-one");
        assert!(!unit.run_on_start);
        assert!(matches!(unit.unit_type, Unit::TransientService(_)));
        let Unit::TransientService(service) = unit.unit_type else {
            panic!("Impossible")
        };
        assert_eq!(service.description, "Such a failure");
        assert_eq!(service.service_type, ServiceType::Simple);
        assert_eq!(service.exec, vec!["/usr/bin/false"].try_into().unwrap());
        assert_eq!(
            service.exec_stop,
            Some(vec!["/usr/bin/true"].try_into().unwrap())
        );
    }

    #[test]
    fn parse_access() {
        let config: Config = yaml_serde::from_str(
            "
--- # Access definition check
access:
  i2c-dev:
    - /bin/foo
    - /usr/bin/libexec/bar
",
        )
        .unwrap();
        assert!(config.overrides.is_none());
        assert!(config.system_unit.is_none());
        assert!(config.user_unit.is_none());
        let access = config.access.unwrap();
        let value: Vec<_> = access
            .i2c_dev
            .iter()
            .map(|p| p.0.to_str().unwrap())
            .collect();
        assert_eq!(value, ["/bin/foo", "/usr/bin/libexec/bar"]);
    }

    #[test]
    fn parse_alt_access() {
        let config: Config = yaml_serde::from_str(
            "
--- # Alternative access definition
access:
  i2c-dev: [\"/bin/foo\", \"/bin/bar\"]
",
        )
        .unwrap();
        assert!(config.overrides.is_none());
        assert!(config.system_unit.is_none());
        assert!(config.user_unit.is_none());
        let access = config.access.unwrap();
        let value: Vec<_> = access
            .i2c_dev
            .iter()
            .map(|p| p.0.to_str().unwrap())
            .collect();
        assert_eq!(value, ["/bin/foo", "/bin/bar"]);
    }

    #[test]
    fn parse_empty_groups() {
        let config: Config = yaml_serde::from_str(
            "
--- # Empty groups
overrides:
access:
system-unit:
user-unit:
",
        )
        .unwrap();
        assert!(config.overrides.is_none());
        assert!(config.system_unit.is_none());
        assert!(config.user_unit.is_none());
        assert!(config.access.is_none());
    }
}
