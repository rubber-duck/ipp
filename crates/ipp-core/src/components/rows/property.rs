//! The row traits implemented by `#[derive(SchemaRow)]` and the Rust value types a
//! row property may hold.

use super::layout::RowsLayout;
use crate::components::dynamic_properties::{DynamicPropertyKind, DynamicValue};
use crate::components::schema::FieldError;
use crate::services::asset_management::AssetSource;
use std::sync::Arc;

/// A plain row struct whose fields are typed properties. Implemented by
/// `#[derive(SchemaRow)]`; property indices follow declaration order.
pub trait SchemaRow: Default {
    /// Property names, kinds, optional flags and hints.
    const LAYOUT: RowsLayout;

    /// Copy one property. `Ok(None)` is an absent optional property. Numeric
    /// reads do not allocate; asset and text reads clone their strings.
    fn property(&self, index: u32) -> Result<Option<DynamicValue>, FieldError>;

    /// Replace one property after checking its kind and value, including a text
    /// property's byte bound.
    fn set_property(&mut self, index: u32, value: DynamicValue) -> Result<(), FieldError>;

    /// Clear an optional property; required properties reject the write.
    fn clear_property(&mut self, index: u32) -> Result<(), FieldError>;

    /// Heap bytes owned by the row's properties.
    fn retained_bytes(&self) -> usize;

    /// Visit present asset properties in layout order.
    fn visit_assets(&self, visit: &mut dyn FnMut(&AssetSource));
}

/// A Rust value type usable as a row property.
pub trait RowPropertyValue: Sized {
    /// Exact storage type.
    const KIND: DynamicPropertyKind;

    /// Copy into the shared typed payload.
    fn to_dynamic(&self) -> DynamicValue;

    /// Check kind and value, then take ownership.
    fn from_dynamic(value: DynamicValue) -> Result<Self, FieldError>;

    /// Borrow an asset selection, when this is an asset value.
    fn asset(&self) -> Option<&AssetSource> {
        None
    }

    /// Heap bytes owned by the value.
    fn heap_bytes(&self) -> usize {
        0
    }
}

/// A required (`T`) or optional (`Option<T>`) row property field.
pub trait RowPropertyField {
    /// Exact storage type.
    const KIND: DynamicPropertyKind;

    /// Whether the property may be absent.
    const OPTIONAL: bool;

    /// Copy the present value.
    fn get(&self) -> Option<DynamicValue>;

    /// Replace the value after checking its kind and value.
    fn set(&mut self, value: DynamicValue) -> Result<(), FieldError>;

    /// Clear an optional value; required properties return [`FieldError::WrongType`].
    fn clear(&mut self) -> Result<(), FieldError>;

    /// Borrow a present asset selection.
    fn asset(&self) -> Option<&AssetSource>;

    /// Heap bytes owned by a present value.
    fn heap_bytes(&self) -> usize;
}

macro_rules! row_value {
    ($ty:ty, $kind:ident, |$value:ident| $to:expr) => {
        impl RowPropertyValue for $ty {
            const KIND: DynamicPropertyKind = DynamicPropertyKind::$kind;

            fn to_dynamic(&self) -> DynamicValue {
                let $value = self;
                $to
            }

            fn from_dynamic(value: DynamicValue) -> Result<Self, FieldError> {
                value.validate()?;
                match value {
                    DynamicValue::$kind(value) => Ok(value),
                    _ => Err(FieldError::WrongType),
                }
            }
        }
    };
}

row_value!(f32, F32, |value| DynamicValue::F32(*value));
row_value!(i32, I32, |value| DynamicValue::I32(*value));
row_value!(u32, U32, |value| DynamicValue::U32(*value));
row_value!(bool, Bool, |value| DynamicValue::Bool(*value));
row_value!([f32; 2], Vec2, |value| DynamicValue::Vec2(*value));
row_value!([f32; 3], Vec3, |value| DynamicValue::Vec3(*value));
row_value!([f32; 4], Vec4, |value| DynamicValue::Vec4(*value));

impl RowPropertyValue for AssetSource {
    const KIND: DynamicPropertyKind = DynamicPropertyKind::Asset;

    fn to_dynamic(&self) -> DynamicValue {
        DynamicValue::Asset(self.clone())
    }

    fn from_dynamic(value: DynamicValue) -> Result<Self, FieldError> {
        value.validate_representation()?;
        match value {
            DynamicValue::Asset(value) => Ok(value),
            _ => Err(FieldError::WrongType),
        }
    }

    fn asset(&self) -> Option<&AssetSource> {
        Some(self)
    }

    fn heap_bytes(&self) -> usize {
        self.uri.len()
    }
}

/// Text rows accept any UTF-8 string here; the derive adds the declared byte
/// bound through [`check_row_text`]. Row text is shared and immutable like
/// component text: a write replaces the reference.
impl RowPropertyValue for Arc<str> {
    const KIND: DynamicPropertyKind = DynamicPropertyKind::Text;

    fn to_dynamic(&self) -> DynamicValue {
        DynamicValue::Text(self.clone())
    }

    fn from_dynamic(value: DynamicValue) -> Result<Self, FieldError> {
        match value {
            DynamicValue::Text(value) => Ok(value),
            _ => Err(FieldError::WrongType),
        }
    }

    fn heap_bytes(&self) -> usize {
        self.len()
    }
}

/// Reject text longer than `max_bytes` UTF-8 bytes; other values pass to the
/// property's own kind check. Called by `#[derive(SchemaRow)]` before every
/// write to a `#[schema(text = N)]` property.
pub fn check_row_text(value: &DynamicValue, max_bytes: u32) -> Result<(), FieldError> {
    match value {
        DynamicValue::Text(text) if text.len() > max_bytes as usize => Err(FieldError::TextTooLong),
        _ => Ok(()),
    }
}

impl<T: RowPropertyValue> RowPropertyField for T {
    const KIND: DynamicPropertyKind = T::KIND;

    const OPTIONAL: bool = false;

    fn get(&self) -> Option<DynamicValue> {
        Some(self.to_dynamic())
    }

    fn set(&mut self, value: DynamicValue) -> Result<(), FieldError> {
        *self = T::from_dynamic(value)?;
        Ok(())
    }

    fn clear(&mut self) -> Result<(), FieldError> {
        Err(FieldError::WrongType)
    }

    fn asset(&self) -> Option<&AssetSource> {
        RowPropertyValue::asset(self)
    }

    fn heap_bytes(&self) -> usize {
        RowPropertyValue::heap_bytes(self)
    }
}

impl<T: RowPropertyValue> RowPropertyField for Option<T> {
    const KIND: DynamicPropertyKind = T::KIND;

    const OPTIONAL: bool = true;

    fn get(&self) -> Option<DynamicValue> {
        self.as_ref().map(RowPropertyValue::to_dynamic)
    }

    fn set(&mut self, value: DynamicValue) -> Result<(), FieldError> {
        *self = Some(T::from_dynamic(value)?);
        Ok(())
    }

    fn clear(&mut self) -> Result<(), FieldError> {
        *self = None;
        Ok(())
    }

    fn asset(&self) -> Option<&AssetSource> {
        self.as_ref().and_then(RowPropertyValue::asset)
    }

    fn heap_bytes(&self) -> usize {
        self.as_ref().map_or(0, RowPropertyValue::heap_bytes)
    }
}
