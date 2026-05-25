// Copyright (c) 2026 Jolla Mobile Ltd

//! TOH interface.

// Currently only used internally by the daemon but would be useful for other things as well.

use crate::errors::*;
use log::warn;
use packed_struct::{PackedStruct, PackingError};
use std::collections::BTreeMap;
use std::num::TryFromIntError;

/// Parsing of memory chip content failed.
#[derive(Debug, Clone)]
pub enum ParseError {
    /// Header did not unpack.
    BadHeader(PackingError),
    /// Magic was not correct.
    BadMagic(WrongMagicError),
    /// Checksum was not correct.
    BadChecksum(ChecksumError),
    /// There is not enough data to parse header or payload.
    NotEnoughData(MissingDataError),
    /// Payload did not parse correctly.
    PayloadParsingError(CBORParsingError),
    /// Payload had wrong type for a known key.
    PayloadWrongType(ciborium::Value),
    /// Payload integer value out of bounds for a known key.
    PayloadOutOfBoundsInteger(TryFromIntError),
    /// Payload unsupported value or type.
    PayloadUnsupportedValue(ExtraValueConversionError),
}

impl From<PackingError> for ParseError {
    fn from(error: PackingError) -> Self {
        ParseError::BadHeader(error)
    }
}

impl From<WrongMagicError> for ParseError {
    fn from(error: WrongMagicError) -> Self {
        ParseError::BadMagic(error)
    }
}

impl From<ChecksumError> for ParseError {
    fn from(error: ChecksumError) -> Self {
        ParseError::BadChecksum(error)
    }
}

impl From<MissingDataError> for ParseError {
    fn from(error: MissingDataError) -> Self {
        ParseError::NotEnoughData(error)
    }
}

impl From<ciborium::de::Error<std::io::Error>> for ParseError {
    fn from(error: ciborium::de::Error<std::io::Error>) -> Self {
        ParseError::PayloadParsingError(error.into())
    }
}

impl From<ciborium::Value> for ParseError {
    fn from(error: ciborium::Value) -> Self {
        ParseError::PayloadWrongType(error)
    }
}

impl From<TryFromIntError> for ParseError {
    fn from(error: TryFromIntError) -> Self {
        ParseError::PayloadOutOfBoundsInteger(error)
    }
}

impl From<ExtraValueConversionError> for ParseError {
    fn from(error: ExtraValueConversionError) -> Self {
        ParseError::PayloadUnsupportedValue(error)
    }
}

/// Values in CBOR data.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ExtraValue {
    // TODO: Fill in the rest of the data types in CBOR
    Boolean(bool),
    I64(i64),
    U64(u64),
    F64(f64),
    Bytes(Vec<u8>),
    Text(String),
    Null,
    Tag(u64, Box<ExtraValue>),
    Array(Vec<ExtraValue>),
    Map(BTreeMap<String, ExtraValue>),
}

impl TryFrom<ciborium::Value> for ExtraValue {
    type Error = ExtraValueConversionError;

    fn try_from(value: ciborium::Value) -> Result<Self, Self::Error> {
        use ExtraValue::*;
        use ExtraValueConversionError::*;
        match value {
            ciborium::Value::Integer(value) => {
                if let Ok(value) = u64::try_from(value) {
                    Ok(U64(value))
                } else if let Ok(value) = i64::try_from(value) {
                    Ok(I64(value))
                } else {
                    Err(UnsupportedIntegerType)
                }
            }
            ciborium::Value::Bytes(value) => Ok(Bytes(value)),
            ciborium::Value::Float(value) => Ok(F64(value)),
            ciborium::Value::Text(value) => Ok(Text(value)),
            ciborium::Value::Bool(value) => Ok(Boolean(value)),
            ciborium::Value::Null => Ok(Null),
            ciborium::Value::Tag(tag, inner_value) => {
                Ok(Tag(tag, Box::new(ExtraValue::try_from(*inner_value)?)))
            }
            ciborium::Value::Array(value) => Ok(Array(
                value
                    .into_iter()
                    .map(ExtraValue::try_from)
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            ciborium::Value::Map(value) => Ok(Map(value
                .into_iter()
                .map(|(inner_key, inner_value)| {
                    if let ciborium::Value::Text(inner_key) = inner_key {
                        Ok((inner_key, ExtraValue::try_from(inner_value)?))
                    } else {
                        Err(MapKeysMustBeStrings)
                    }
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?)),
            _ => Err(UnsupportedValueType),
        }
    }
}

/// Error for non-representable values from ExtraValue into zvariant::OwnedValue.
///
/// Can be converted into &ExtraValue.
#[derive(Debug, Clone)]
pub struct NonConvertableValue<'v>(&'v ExtraValue);

impl ExtraValue {
    // TODO: It would be nice if library users didn't have to depend on zvariant unnecessarily.
    /// Converts to zvariant::Value but discards inner values that could not be converted.
    pub fn try_into_dbus_lossy(&self) -> Result<zvariant::Value<'_>, NonConvertableValue<'_>> {
        use zvariant::Value;
        use ExtraValue::*;
        match self {
            Boolean(value) => Ok(Value::Bool(*value)),
            I64(value) => Ok(Value::I64(*value)),
            U64(value) => Ok(Value::U64(*value)),
            F64(value) => Ok(Value::F64(*value)),
            Bytes(array) => Ok(Value::Array(array.into())),
            Text(string) => Ok(Value::Str(string.into())),
            value @ Null => Err(NonConvertableValue(value)),
            value @ Tag(_tag, _value) => Err(NonConvertableValue(value)),
            Array(array) => Ok(Value::Array(
                array
                    .iter()
                    .filter_map(|v| v.try_into_dbus_lossy().ok())
                    .collect::<Vec<Value>>()
                    .into(),
            )),
            Map(map) => Ok(Value::Dict(
                map.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.try_into_dbus_lossy().ok()?)))
                    .collect::<BTreeMap<String, Value>>()
                    .into(),
            )),
        }
    }
}

impl<'v> From<NonConvertableValue<'v>> for &'v ExtraValue {
    fn from(value: NonConvertableValue<'v>) -> Self {
        value.0
    }
}

// TODO: How about chipless TOHs? Perhaps we need an enum
/// TOH memory chip content.
#[derive(Debug, Default, PartialEq)]
pub struct Info {
    // TODO: Non-exhaustive?
    pub vendor_id: u16,
    pub product_id: u16,
    pub schema_version: Option<u8>,
    pub serial_number: Option<String>,
    pub vendor_name: Option<String>,
    pub product_name: Option<String>,
    pub vendor_website: Option<String>,
    pub product_website: Option<String>,
    pub leave_power_on: Option<bool>,
    pub power_input_toh: Option<bool>,
    // Insert any known keys from data before this
    pub extra: BTreeMap<String, ExtraValue>,
}

mod header {
    use packed_struct::prelude::*;

    #[derive(PackedStruct)]
    #[packed_struct(endian = "msb")]
    pub(crate) struct Header {
        pub(crate) magic: [u8; 4],
        pub(crate) checksum: u32,
        pub(crate) vendor_id: u16,
        pub(crate) product_id: u16,
        pub(crate) reserved: u16,
        pub(crate) payload_size: u16,
    }
}

impl Info {
    /// Parse from plain data.
    pub fn parse_from_bytes(content: &[u8]) -> Result<Self, ParseError> {
        if content.len() < 16 {
            return Err(MissingDataError {
                length: content.len(),
                expected: 16,
            }
            .into());
        }
        let header::Header {
            magic,
            checksum,
            vendor_id,
            product_id,
            reserved,
            payload_size,
        } = header::Header::unpack(content[0..16].try_into().unwrap())?;
        if magic != *b"JTOH" {
            return Err(WrongMagicError { value: magic }.into());
        }
        if reserved != 0 {
            warn!("Reserved bits are not zero");
        }
        let end_of_payload: usize = (0x10 + payload_size).into();
        if content.len() < end_of_payload {
            return Err(MissingDataError {
                length: content.len(),
                expected: end_of_payload,
            }
            .into());
        }
        let calculated = crc32fast::hash(&content[0x08..end_of_payload]);
        if checksum != calculated {
            return Err(ChecksumError {
                calculated,
                expected: checksum,
            }
            .into());
        }

        let mut payload: BTreeMap<String, ciborium::Value> =
            ciborium::from_reader(&content[0x10..end_of_payload])?;
        let schema_version = payload
            .remove("SC")
            .map(|value| value.into_integer())
            .transpose()?
            .map(u8::try_from)
            .transpose()?;
        let serial_number = payload
            .remove("SN")
            .map(|value| value.into_text())
            .transpose()?;
        let vendor_name = payload
            .remove("VN")
            .map(|value| value.into_text())
            .transpose()?;
        let product_name = payload
            .remove("PN")
            .map(|value| value.into_text())
            .transpose()?;
        let vendor_website = payload
            .remove("VS")
            .map(|value| value.into_text())
            .transpose()?;
        let product_website = payload
            .remove("PS")
            .map(|value| value.into_text())
            .transpose()?;
        let leave_power_on = payload
            .remove("PO")
            .map(|value| value.into_bool())
            .transpose()?;
        let power_input_toh = payload
            .remove("PI")
            .map(|value| value.into_bool())
            .transpose()?;

        Ok(Info {
            vendor_id,
            product_id,
            schema_version,
            serial_number,
            product_name,
            vendor_name,
            product_website,
            vendor_website,
            leave_power_on,
            power_input_toh,
            extra: payload
                .into_iter()
                .map(|(key, value)| Ok((key, ExtraValue::try_from(value)?)))
                .collect::<Result<BTreeMap<_, _>, ExtraValueConversionError>>()?,
        })
    }
}

/// Trait for is_powered method.
pub trait IsPowered {
    /// Check if TOH connector power out is on.
    fn is_powered(&mut self) -> std::io::Result<bool>;
}

/// Trait for is_present method.
pub trait IsPresent {
    /// Check if there is a TOH present.
    fn is_present(&mut self) -> std::io::Result<bool>;
}

/// Error during TOH detection.
#[derive(Debug)]
pub enum DetectionError {
    /// IO error happened.
    Io(std::io::Error),
    /// Parsing error happened.
    Parse(ParseError),
}

impl From<std::io::Error> for DetectionError {
    fn from(error: std::io::Error) -> Self {
        DetectionError::Io(error)
    }
}

impl From<ParseError> for DetectionError {
    fn from(error: ParseError) -> Self {
        DetectionError::Parse(error)
    }
}

/// Trait for detect method.
pub trait Detect {
    /// Detect presence of TOH and fetch TOH info.
    ///
    /// Returns None if no TOH is present. If TOH is present returns Some with Info that may be
    /// populated from device or cache.
    fn detect(&mut self) -> Result<Option<Info>, DetectionError>;
}
