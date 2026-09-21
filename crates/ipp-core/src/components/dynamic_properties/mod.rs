//! Named component properties, independent of shader requirements and GPU layout.

mod storage;
mod value;

pub use storage::{
    DYNAMIC_METADATA, DynamicProperties, DynamicPropertyDescriptor, is_dynamic_field,
};
pub use value::{DynamicPropertyKind, DynamicValue};
