// Copyright (c) 2026 Jolla Mobile Ltd

//! Types for parsing config files.

use crate::toh::ExtraValue;
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Exec {
    bin: PathBuf,
    args: Vec<String>,
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
            let bin: PathBuf = bin.into();
            let mut args = if let Some(capacity) = access.size_hint() {
                Vec::with_capacity(capacity)
            } else {
                Vec::new()
            };
            while let Some(arg) = access.next_element::<String>()? {
                args.push(arg);
            }
            Ok(Self::Value { bin, args })
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

/// Missing executable path.
#[derive(Debug, Error)]
#[error("Missing executable path")]
pub(crate) struct MissingExecutablePath;

impl TryFrom<Vec<&str>> for Exec {
    type Error = MissingExecutablePath;

    fn try_from(vec: Vec<&str>) -> Result<Self, Self::Error> {
        // TODO: Must check that this is absolute!
        let mut it = vec.into_iter();
        if let Some(bin) = it.next() {
            Ok(Self {
                bin: bin.into(),
                args: Vec::from_iter(it.map(ToOwned::to_owned)),
            })
        } else {
            Err(MissingExecutablePath)
        }
    }
}

impl Exec {
    pub(crate) fn path(&self) -> &str {
        self.bin
            .to_str()
            .expect("Validity as string was checked during parsing")
    }

    pub(crate) fn args(&self) -> Vec<&str> {
        let (_, bin) = self
            .bin
            .to_str()
            .expect("Validity as string was checked during parsing")
            .rsplit_once('/')
            .expect("Binary path contains at least one '/'");
        std::iter::once(bin)
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
    }
}
