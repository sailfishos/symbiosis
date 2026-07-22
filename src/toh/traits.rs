// Copyright (c) 2026 Jolla Mobile Ltd

//! TOH interface.

use super::info::*;

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
