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
