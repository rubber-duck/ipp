//! Typed field replacement and streaming target contract primitives.

use crate::EntityId;

/// Component derive, compiled for the macro host and evaluated for the host target.
pub use ipp_schema_derive::SchemaComponent;

pub use super::lifecycle::ComponentAssetReference;
pub(crate) use super::lifecycle::ComponentLifecycle;

/// Types supported by exact field dispatch and the binary value codec.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FieldKind {
    /// Typed dynamic component property.
    Dynamic = 11,
    /// Finite IEEE754 single precision.
    F32 = 1,
    /// Generational entity identity.
    Entity = 2,
    /// Unsigned 32-bit integer.
    U32 = 3,
    /// Unsigned 64-bit integer.
    U64 = 4,
    /// Owned UTF-8 string.
    String = 5,
    /// Owned byte collection.
    Bytes = 6,
    /// Canonical boolean flag.
    Bool = 7,
}

/// Fully owned, resolved field replacement.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
    /// Validated named-property payload.
    Dynamic(crate::DynamicValue),
    /// Numeric replacement.
    F32(f32),
    /// Persistent identity, never a batch alias.
    Entity(EntityId),
    /// Integer replacement.
    U32(u32),
    /// Integer replacement.
    U64(u64),
    /// Owned text replacement.
    String(String),
    /// Owned collection replacement.
    Bytes(Vec<u8>),
    /// Boolean replacement.
    Bool(bool),
}

impl FieldValue {
    /// Wire value kind.
    pub fn kind(&self) -> FieldKind {
        match self {
            Self::Dynamic(_) => FieldKind::Dynamic,
            Self::F32(_) => FieldKind::F32,
            Self::Entity(_) => FieldKind::Entity,
            Self::U32(_) => FieldKind::U32,
            Self::U64(_) => FieldKind::U64,
            Self::String(_) => FieldKind::String,
            Self::Bytes(_) => FieldKind::Bytes,
            Self::Bool(_) => FieldKind::Bool,
        }
    }
}

/// Exact dispatch rejects unknown IDs, non-field offsets, and incompatible values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldError {
    /// Omitted or unknown component.
    UnknownComponent,
    /// Offset is not the start of an exposed field.
    UnknownField,
    /// Value type does not match the field type.
    WrongType,
    /// Floating point values must be finite.
    NonFinite,
    /// The type has no compiled authored creation factory.
    CreationUnavailable,
}

/// A byte consumer used by both compatibility hashing and optional export.
pub trait ContractSink {
    /// Consume bytes in canonical order.
    fn write(&mut self, bytes: &[u8]);
}

impl ContractSink for Vec<u8> {
    fn write(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }
}

/// Streaming FNV-1a 64 compatibility identity (not an authentication hash).
pub struct ContractHash(pub u64);

impl Default for ContractHash {
    fn default() -> Self {
        Self(0xcbf29ce484222325)
    }
}

impl ContractSink for ContractHash {
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(0x100000001b3);
        }
    }
}

/// Canonical UTF-8 with a little-endian u32 byte length.
pub fn write_string(sink: &mut impl ContractSink, value: &str) {
    sink.write(&(value.len() as u32).to_le_bytes());
    sink.write(value.as_bytes());
}

/// Per-type implementations emitted by the derive; no descriptive tree traversal.
pub trait SchemaComponent: Sized {
    /// Number of exposed fields, independent of target padding.
    const FIELD_COUNT: usize;

    /// Optional authored creation contract, independent of field access.
    fn create() -> Option<Self>;

    /// Check an exposed offset without requiring a value or creation factory.
    fn has_field(offset: u32) -> bool;

    /// Own exposed values, omitting internal fields.
    fn fields(&self) -> Vec<(u32, FieldValue)>;

    /// Read one exposed field without enumerating or cloning unrelated values.
    fn field(&self, offset: u32) -> Result<FieldValue, FieldError>;

    /// Heap allocation retained by exposed owned fields.
    fn retained_bytes(&self) -> Option<usize>;

    /// Apply a typed replacement at an exact target field offset.
    fn set_field(&mut self, offset: u32, value: FieldValue) -> Result<(), FieldError>;

    /// Check the exact exposed field and operation type.
    fn validate_field(offset: u32, kind: FieldKind) -> Result<(), FieldError>;

    /// Stream target size/alignment, exposed offsets/types, and actual defaults.
    fn write_contract(sink: &mut impl ContractSink);
}

/// Sealed-by-convention primitive field codec used by the derive.
pub trait SchemaField: Sized {
    /// Copy or clone into the owned inspection value.
    fn to_value(&self) -> FieldValue;

    /// Corresponding owned value tag.
    const KIND: FieldKind;

    /// Type-check an owned value.
    fn from_value(value: FieldValue) -> Result<Self, FieldError>;

    /// Stream the actual creation default in wire representation.
    fn write_default(&self, sink: &mut impl ContractSink);

    /// Heap allocation retained by this value.
    fn retained_bytes(&self) -> Option<usize> {
        Some(0)
    }

    /// Validate replacement kind.
    fn validate_kind(kind: FieldKind) -> Result<(), FieldError> {
        if kind == Self::KIND {
            Ok(())
        } else {
            Err(FieldError::WrongType)
        }
    }
}

macro_rules! primitive {
    ($ty:ty,$variant:ident) => {
        impl SchemaField for $ty {
            const KIND: FieldKind = FieldKind::$variant;

            fn to_value(&self) -> FieldValue {
                FieldValue::$variant(*self)
            }

            fn from_value(value: FieldValue) -> Result<Self, FieldError> {
                if let FieldValue::$variant(v) = value {
                    Ok(v)
                } else {
                    Err(FieldError::WrongType)
                }
            }

            fn write_default(&self, sink: &mut impl ContractSink) {
                sink.write(&self.to_le_bytes());
            }
        }
    };
}
primitive!(u32, U32);
primitive!(u64, U64);

impl SchemaField for bool {
    const KIND: FieldKind = FieldKind::Bool;

    fn to_value(&self) -> FieldValue {
        FieldValue::Bool(*self)
    }

    fn from_value(value: FieldValue) -> Result<Self, FieldError> {
        match value {
            FieldValue::Bool(value) => Ok(value),
            _ => Err(FieldError::WrongType),
        }
    }

    fn write_default(&self, sink: &mut impl ContractSink) {
        sink.write(&[u8::from(*self)]);
    }
}

impl SchemaField for f32 {
    fn to_value(&self) -> FieldValue {
        FieldValue::F32(*self)
    }
    const KIND: FieldKind = FieldKind::F32;

    fn from_value(value: FieldValue) -> Result<Self, FieldError> {
        match value {
            FieldValue::F32(v) if v.is_finite() => Ok(v),
            FieldValue::F32(_) => Err(FieldError::NonFinite),
            _ => Err(FieldError::WrongType),
        }
    }

    fn write_default(&self, sink: &mut impl ContractSink) {
        sink.write(&self.to_le_bytes());
    }
}

impl SchemaField for EntityId {
    fn to_value(&self) -> FieldValue {
        FieldValue::Entity(*self)
    }
    const KIND: FieldKind = FieldKind::Entity;

    fn from_value(value: FieldValue) -> Result<Self, FieldError> {
        if let FieldValue::Entity(v) = value {
            Ok(v)
        } else {
            Err(FieldError::WrongType)
        }
    }

    fn write_default(&self, sink: &mut impl ContractSink) {
        sink.write(&self.to_bits().to_le_bytes());
    }
}

impl SchemaField for String {
    fn to_value(&self) -> FieldValue {
        FieldValue::String(self.clone())
    }
    const KIND: FieldKind = FieldKind::String;

    fn from_value(value: FieldValue) -> Result<Self, FieldError> {
        if let FieldValue::String(v) = value {
            Ok(v)
        } else {
            Err(FieldError::WrongType)
        }
    }

    fn write_default(&self, sink: &mut impl ContractSink) {
        write_string(sink, self);
    }

    fn retained_bytes(&self) -> Option<usize> {
        Some(self.capacity())
    }
}

impl SchemaField for Vec<u8> {
    fn to_value(&self) -> FieldValue {
        FieldValue::Bytes(self.clone())
    }
    const KIND: FieldKind = FieldKind::Bytes;

    fn from_value(value: FieldValue) -> Result<Self, FieldError> {
        if let FieldValue::Bytes(v) = value {
            Ok(v)
        } else {
            Err(FieldError::WrongType)
        }
    }

    fn write_default(&self, sink: &mut impl ContractSink) {
        sink.write(&(self.len() as u32).to_le_bytes());
        sink.write(self);
    }

    fn retained_bytes(&self) -> Option<usize> {
        Some(self.capacity())
    }
}
