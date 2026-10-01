//! Dedicated schema kinds for exact runtime composition references.

use super::{OutputRef, OutputTarget, WorldRef};
use crate::components::schema::{ContractSink, FieldError, FieldKind, FieldValue, SchemaField};

impl SchemaField for Option<WorldRef> {
    const KIND: FieldKind = FieldKind::World;

    fn to_value(&self) -> FieldValue {
        FieldValue::World(*self)
    }

    fn from_value(value: FieldValue) -> Result<Self, FieldError> {
        match value {
            FieldValue::World(value) => Ok(value),
            _ => Err(FieldError::WrongType),
        }
    }

    fn write_default(&self, sink: &mut impl ContractSink) {
        sink.write(&[u8::from(self.is_some())]);
        if let Some(value) = self {
            sink.write(&value.id.0.to_le_bytes());
            sink.write(&(value.incarnation as u64).to_le_bytes());
        }
    }
}

impl SchemaField for Option<OutputRef> {
    const KIND: FieldKind = FieldKind::Output;

    fn to_value(&self) -> FieldValue {
        FieldValue::Output(*self)
    }

    fn from_value(value: FieldValue) -> Result<Self, FieldError> {
        match value {
            FieldValue::Output(value) => Ok(value),
            _ => Err(FieldError::WrongType),
        }
    }

    fn write_default(&self, sink: &mut impl ContractSink) {
        sink.write(&[u8::from(self.is_some())]);
        if let Some(value) = self {
            Some(value.world).write_default(sink);
            match value.target {
                OutputTarget::Canvas => sink.write(&[0]),
                OutputTarget::Camera {
                    entity,
                    incarnation,
                } => {
                    sink.write(&[1]);
                    sink.write(&entity.to_bits().to_le_bytes());
                    sink.write(&incarnation.to_le_bytes());
                }
            }
        }
    }
}
