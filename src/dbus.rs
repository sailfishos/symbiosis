// Copyright (c) 2026 Jolla Mobile Ltd

//! D-Bus specific stuff.

use crate::toh::Info;
use std::collections::HashMap;
use zbus::interface;
use zvariant::{Optional, OwnedValue};

/// Representation of TOH on D-Bus.
pub struct Toh {
    info: Info,
}

impl Toh {
    /// Create new representation from TOH info.
    pub fn new(info: Info) -> Self {
        Self { info }
    }
}

#[interface(name = "org.sailfishos.tohd1.Toh")]
impl Toh {
    #[zbus(property)]
    fn vendor_id(&self) -> u16 {
        self.info.vendor_id
    }

    #[zbus(property)]
    fn product_id(&self) -> u16 {
        self.info.product_id
    }

    #[zbus(property)]
    fn schema_version(&self) -> Optional<u8> {
        self.info.schema_version.into()
    }

    #[zbus(property)]
    fn serial_number(&self) -> Optional<String> {
        self.info.serial_number.clone().into()
    }

    #[zbus(property)]
    fn vendor_name(&self) -> Optional<String> {
        self.info.vendor_name.clone().into()
    }

    #[zbus(property)]
    fn product_name(&self) -> Optional<String> {
        self.info.product_name.clone().into()
    }

    #[zbus(property)]
    fn vendor_website(&self) -> Optional<String> {
        self.info.vendor_website.clone().into()
    }

    #[zbus(property)]
    fn product_website(&self) -> Optional<String> {
        self.info.product_website.clone().into()
    }

    #[zbus(property)]
    fn leave_power_on(&self) -> Optional<bool> {
        self.info.leave_power_on.into()
    }

    #[zbus(property)]
    fn power_input_toh(&self) -> Optional<bool> {
        self.info.power_input_toh.into()
    }

    #[zbus(property)]
    fn extra_data(&self) -> HashMap<String, OwnedValue> {
        self.info
            .extra
            .iter()
            .filter_map(|(k, v)| Some((k, v.try_into_dbus_lossy().ok()?)))
            .map(|(k, v)| {
                (
                    k.clone(),
                    v.try_to_owned().expect("None of the values is an fd here"),
                )
            })
            .collect::<HashMap<_, _>>()
    }

    // TODO: Implement an interface to hand out access to the i2c-dev device
}
