// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! Error types.

use thiserror::Error;

/// Wrong magic value encountered.
#[derive(Debug, Clone, Error)]
#[error("Unexpected magic value: {value:?}")]
pub struct WrongMagicError {
    /// Encountered value in the data.
    pub value: [u8; 4],
}

/// Calculated checksum did not match the expected value.
#[derive(Debug, Clone, Error)]
#[error("Checksum did not match: {calculated} != {expected}")]
pub struct ChecksumError {
    /// Calculated value.
    pub calculated: u32,
    /// Expected value in the data.
    pub expected: u32,
}

/// Less data received than expected.
#[derive(Debug, Clone, Error)]
#[error("Missing data: got {length} bytes, expected {expected} bytes")]
pub struct MissingDataError {
    /// Length of the data received.
    pub length: usize,
    /// Expected length of the data.
    pub expected: usize,
}

// NB: This mainly exists because ciborium::de::Error is not Clone.
/// CBOR payload parsing failed.
///
/// Mirrors [`ciborium::de::Error`].
#[derive(Debug, Clone, Error)]
pub enum CBORParsingError {
    /// IO failure.
    #[error("IO error while parsing CBOR")]
    Io,
    /// Syntax error at offset.
    #[error("Syntax error in CBOR at offset {0}")]
    Syntax(usize),
    /// Semantic error with description and offset.
    #[error("Semantic error in CBOR: {0}")]
    Semantic(String, Option<usize>),
    /// Serde tried to recurse too much.
    #[error("Recursion limit exceeded in CBOR parsing")]
    RecursionLimitExceeded,
}

impl From<ciborium::de::Error<std::io::Error>> for CBORParsingError {
    fn from(error: ciborium::de::Error<std::io::Error>) -> Self {
        use ciborium::de::Error::*;
        match error {
            Io(_error) => CBORParsingError::Io,
            Syntax(offset) => CBORParsingError::Syntax(offset),
            Semantic(offset, text) => CBORParsingError::Semantic(text, offset),
            RecursionLimitExceeded => CBORParsingError::RecursionLimitExceeded,
        }
    }
}

/// Failed to convert some payload or config data.
#[derive(Debug, Clone, Error)]
pub enum ExtraValueConversionError {
    /// Integer type is not yet supported.
    #[error("Unsupported integer type")]
    UnsupportedIntegerType,
    /// Map keys were not all strings.
    #[error("Some map keys were not strings")]
    MapKeysMustBeStrings,
    /// Unsupported value type encountered.
    #[error("Unsupported value type")]
    UnsupportedValueType,
}
