// Copyright (c) 2026 Jolla Mobile Ltd

//! TOH info.

use super::config::{ConfigError, Configs, Overrides};
use super::value::*;
use crate::errors::*;
use log::warn;
use packed_struct::{PackedStruct, PackingError};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::num::TryFromIntError;
use thiserror::Error;

/// Parsing of memory chip content failed.
#[derive(Debug, Clone, Error)]
pub enum ParseError {
    /// Header did not unpack.
    #[error("Bad header: {0}")]
    BadHeader(#[from] PackingError),
    /// Magic was not correct.
    #[error("Bad magic value: {0}")]
    BadMagic(#[from] WrongMagicError),
    /// Checksum was not correct.
    #[error("Bad magic value: {0}")]
    BadChecksum(#[from] ChecksumError),
    /// There is not enough data to parse header or payload.
    #[error("Not enough data to parse: {0}")]
    NotEnoughData(#[from] MissingDataError),
    /// Payload did not parse correctly.
    #[error("CBOR parsing failed: {0}")]
    PayloadParsingError(CBORParsingError),
    /// Payload had wrong type for a known key.
    #[error("Payload had unexpected type for a known key, value: {0:?}")]
    PayloadWrongType(ciborium::Value),
    /// Payload integer value out of bounds for a known key.
    #[error("Integer in payload is out of bounds for a known key: {0}")]
    PayloadOutOfBoundsInteger(#[from] TryFromIntError),
    /// Payload unsupported value or type.
    #[error("Unsupported value in payload: {0}")]
    PayloadUnsupportedValue(#[from] ExtraValueConversionError),
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

// TODO: How about chipless TOHs? Perhaps we need an enum
/// TOH memory chip content.
#[derive(Debug, Default, Deserialize, PartialEq)]
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
    #[serde(flatten)]
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

    /// Returns payload content.
    ///
    /// Set include_extra to false to skip all extra keys.
    fn get_payload(&self, include_extra: bool) -> BTreeMap<String, ciborium::Value> {
        use ciborium::Value;
        let mut payload = BTreeMap::<String, Value>::new();
        let Info {
            serial_number,
            vendor_name,
            product_name,
            vendor_website,
            product_website,
            leave_power_on,
            power_input_toh,
            extra,
            ..
        } = self;
        if let Some(value) = serial_number {
            payload.insert("SN".to_owned(), value.as_str().into());
        }
        if let Some(value) = vendor_name {
            payload.insert("VN".to_owned(), value.as_str().into());
        }
        if let Some(value) = product_name {
            payload.insert("PN".to_owned(), value.as_str().into());
        }
        if let Some(value) = vendor_website {
            payload.insert("VS".to_owned(), value.as_str().into());
        }
        if let Some(value) = product_website {
            payload.insert("PS".to_owned(), value.as_str().into());
        }
        if let Some(value) = leave_power_on {
            payload.insert("PO".to_owned(), (*value).into());
        }
        if let Some(value) = power_input_toh {
            payload.insert("PI".to_owned(), (*value).into());
        }
        if !payload.is_empty() {
            // Zero is the only existing version payload schema.
            payload.insert("SC".to_owned(), 0.into());
        }

        if include_extra {
            for (key, value) in extra.iter() {
                payload.insert(key.to_owned(), value.clone().into_cbor());
            }
        }

        payload
    }

    /// Turn TOH info into bytes that can be written onto a memory chip.
    pub fn into_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut buff = Cursor::new(Vec::with_capacity(16));

        // Write the header
        buff.write_all(&[0x4A, 0x54, 0x4F, 0x48])?;
        buff.write_all(&0_u32.to_be_bytes())?; // Placeholder for checksum
        buff.write_all(&self.vendor_id.to_be_bytes())?;
        buff.write_all(&self.product_id.to_be_bytes())?;
        buff.write_all(&0_u16.to_be_bytes())?; // Padding
        buff.write_all(&0_u16.to_be_bytes())?; // Zero for size
        assert!(buff.get_ref().len() == 16);

        // If we have a payload write that too and update size
        let payload = self.get_payload(true);
        if !payload.is_empty() {
            ciborium::into_writer(&payload, &mut buff).map_err(|error| {
                use ciborium::ser::Error;
                match error {
                    Error::Io(error) => error,
                    Error::Value(error) => std::io::Error::other(error),
                }
            })?;

            // Update size field
            let size = (buff.get_ref().len() - 16) as u16;
            buff.set_position(0x0e);
            buff.write_all(&size.to_be_bytes())?;
        }

        // Update checksum
        let data = buff.get_ref();
        let checksum = crc32fast::hash(&data[0x08..]);
        buff.set_position(0x04);
        buff.write_all(&checksum.to_be_bytes())?;
        Ok(buff.into_inner())
    }

    /// Read configs for this TOH.
    pub fn read_configs(&self) -> Result<Option<Configs>, ConfigError> {
        Configs::find(self.vendor_id, self.product_id)
    }

    /// Apply overrides from configs to this Info instance.
    ///
    /// Consumes the Overrides instance, clone it if you need to apply it multiple times for some
    /// reason.
    pub fn apply_overrides(&mut self, overrides: Overrides) {
        overrides.apply_overrides(self);
    }
}
