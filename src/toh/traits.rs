// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! TOH interface traits.

use super::info::*;
use async_trait::async_trait;

/// Trait for [`is_powered`](Self::is_powered) method.
#[async_trait]
pub trait IsPowered {
    type Error;

    /// Check if TOH connector power out is on.
    async fn is_powered(&mut self) -> Result<bool, Self::Error>;
}

/// Trait for [`is_present`](Self::is_present) method.
#[async_trait]
pub trait IsPresent {
    type Error;

    /// Check if there is a TOH present.
    async fn is_present(&mut self) -> Result<bool, Self::Error>;
}

/// Trait for [`detect`](Self::detect) method.
#[async_trait]
pub trait Detect {
    type Error;

    /// Detect presence of TOH and fetch TOH info.
    ///
    /// Returns [`None`] if no TOH is present. If TOH is present returns [`Some`] with [`Info`] that
    /// may be populated from device or cache.
    async fn detect(&mut self) -> Result<Option<Info>, Self::Error>;
}
