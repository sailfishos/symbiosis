// SPDX-FileCopyrightText: 2026 Jolla Mobile Ltd
//
// SPDX-License-Identifier: BSD-3-Clause

//! Values in TOH info.

use crate::errors::*;
use derive_more::From;
use serde::Deserialize;
use std::collections::BTreeMap;

/// Values in CBOR data.
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize, PartialEq, From)]
pub enum ExtraValue {
    // TODO: Fill in the rest of the data types in CBOR and Yaml
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

impl TryFrom<yaml_serde::Value> for ExtraValue {
    type Error = ExtraValueConversionError;

    fn try_from(value: yaml_serde::Value) -> Result<Self, Self::Error> {
        use ExtraValue::*;
        use ExtraValueConversionError::*;
        match value {
            yaml_serde::Value::Number(value) => {
                if let Some(value) = value.as_u64() {
                    Ok(U64(value))
                } else if let Some(value) = value.as_i64() {
                    Ok(I64(value))
                } else if let Some(value) = value.as_f64() {
                    Ok(F64(value))
                } else {
                    Err(UnsupportedIntegerType)
                }
            }
            yaml_serde::Value::String(value) => Ok(Text(value)),
            yaml_serde::Value::Bool(value) => Ok(Boolean(value)),
            yaml_serde::Value::Null => Ok(Null),
            yaml_serde::Value::Sequence(value) => Ok(Array(
                value
                    .into_iter()
                    .map(ExtraValue::try_from)
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            yaml_serde::Value::Mapping(value) => Ok(Map(value
                .into_iter()
                .map(|(inner_key, inner_value)| {
                    if let yaml_serde::Value::String(inner_key) = inner_key {
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

impl<'a> TryFrom<&zvariant::Value<'a>> for ExtraValue {
    type Error = ExtraValueConversionError;

    fn try_from(value: &zvariant::Value<'a>) -> Result<Self, Self::Error> {
        use ExtraValue::*;
        use ExtraValueConversionError::*;
        match value {
            zvariant::Value::Bool(inner) => Ok(Boolean(*inner)),
            zvariant::Value::U8(inner) => Ok(U64((*inner).into())),
            zvariant::Value::I16(inner) => Ok(I64((*inner).into())),
            zvariant::Value::U16(inner) => Ok(U64((*inner).into())),
            zvariant::Value::I32(inner) => Ok(I64((*inner).into())),
            zvariant::Value::U32(inner) => Ok(U64((*inner).into())),
            zvariant::Value::I64(inner) => Ok(I64(*inner)),
            zvariant::Value::U64(inner) => Ok(U64(*inner)),
            zvariant::Value::F64(inner) => Ok(F64(*inner)),
            zvariant::Value::Str(inner) => Ok(Text(inner.as_str().to_owned())),
            zvariant::Value::Value(inner) => (&**inner).try_into(),
            zvariant::Value::Array(inner) => Ok(Array(
                inner
                    .iter()
                    .map(|value| value.try_into())
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            zvariant::Value::Dict(inner) => Ok(Map(inner
                .iter()
                .map(|(key, value)| {
                    if let zvariant::Value::Str(key) = key {
                        Ok((key.as_str().to_owned(), ExtraValue::try_from(value)?))
                    } else {
                        Err(MapKeysMustBeStrings)
                    }
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?)),
            _ => Err(UnsupportedValueType),
        }
    }
}

/// Error for non-representable values from [`ExtraValue`] into [`struct@zvariant::OwnedValue`].
///
/// Can be converted into [`&ExtraValue`](ExtraValue).
#[derive(Debug, Clone)]
pub struct NonConvertableValue<'v>(&'v ExtraValue);

impl ExtraValue {
    // TODO: It would be nice if library users didn't have to depend on zvariant unnecessarily.
    /// Converts to [`enum@zvariant::Value`] but discards inner values that could not be converted.
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

    /// Converts to [`ciborium::Value`].
    pub fn into_cbor(self) -> ciborium::Value {
        use ciborium::Value;
        use ExtraValue::*;
        match self {
            Boolean(value) => value.into(),
            I64(value) => value.into(),
            U64(value) => value.into(),
            F64(value) => value.into(),
            Bytes(array) => array.as_slice().into(),
            Text(string) => string.as_str().into(),
            Null => Value::Null,
            Tag(tag, value) => Value::Tag(tag, Box::new(value.into_cbor())),
            Array(array) => array
                .into_iter()
                .map(|value| value.into_cbor())
                .collect::<Vec<_>>()
                .into(),
            Map(map) => map
                .into_iter()
                .map(|(key, value)| (key.into(), value.into_cbor()))
                .collect::<Vec<(_, _)>>()
                .into(),
        }
    }
}

impl<'v> From<NonConvertableValue<'v>> for &'v ExtraValue {
    fn from(value: NonConvertableValue<'v>) -> Self {
        value.0
    }
}
