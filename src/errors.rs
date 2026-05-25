// Copyright (c) 2026 Jolla Mobile Ltd

//! Error types.

/// Wrong magic errors.
#[derive(Debug, Clone)]
pub struct WrongMagicError {
    pub value: [u8; 4],
}

/// Checksum errors.
#[derive(Debug, Clone)]
pub struct ChecksumError {
    pub calculated: u32,
    pub expected: u32,
}

/// Less data received than expected.
#[derive(Debug, Clone)]
pub struct MissingDataError {
    pub length: usize,
    pub expected: usize,
}

// NB: This mainly exists because ciborium::de::Error is not Clone.
/// CBOR payload parsing failed.
///
/// Mirrors ciborium::de::Error.
#[derive(Debug, Clone)]
pub enum CBORParsingError {
    /// IO failure.
    Io,
    /// Syntax error at offset.
    Syntax(usize),
    /// Semantic error with description and offset.
    Semantic(String, Option<usize>),
    /// Serde tried to recurse too much.
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

/// Failed to convert some payload data.
#[derive(Debug, Clone)]
pub enum ExtraValueConversionError {
    /// Integer type is not yet supported.
    UnsupportedIntegerType,
    /// Map keys were not all strings.
    MapKeysMustBeStrings,
    /// Unsupported value type encountered.
    UnsupportedValueType,
}
