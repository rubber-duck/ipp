//! Compiled schema rows: typed tables whose properties are addressed through
//! the component field-offset space.
//!
//! A component declares `#[schema(rows)] name: Rows<R>` where `R` derives
//! [`SchemaRow`]. Rows field `k` (declaration order among the component's rows
//! fields) owns the virtual region starting at [`row_region_base`]`(k)`; a
//! property address is `region_base + slot * property_count + property_index`.
//! Regions lie below the dynamic-property namespace and above every real struct
//! offset. The whole table is exposed once, at the field's real struct offset,
//! as [`FieldValue::Rows`] in the table encoding documented on [`Rows::encode`].
//!
//! Each slot is unallocated, live or dead. Callers may insert at any
//! unallocated slot, in any order; [`Rows::remove`] makes one slot dead and
//! [`Rows::remove_slots`] makes any number dead in one pass, and a dead slot is
//! never reused within a component incarnation. Bindings hold
//! `(component binding, slot, property)`, never a pointer into the table.
//!
//! Properties are scalars, vectors, asset references or bounded text. A text
//! property is `Arc<str>` or `Option<Arc<str>>` declaring `#[schema(text = N)]`, its
//! UTF-8 byte bound of at most [`MAX_ROW_TEXT_BYTES`]; the layout and target
//! contract carry the bound. Row text travels as a string field value at its
//! property address and as [`DynamicValue::Text`] inside the row API; it is
//! never a dynamic property or an animation target.
//!
//! Dead slots are tracked in memory only; the table encoding carries
//! [`Rows::next_slot`] and the live rows. A decoded table therefore has no dead
//! slots. That is sound because decoding a table (restore or a whole-table write)
//! starts a new component incarnation: no binding survives it, so previously dead
//! slots may be allocated again without any binding observing the reuse.
//!
//! [`FieldValue::Rows`]: crate::components::schema::FieldValue::Rows
//! [`DynamicValue::Text`]: crate::components::dynamic_properties::DynamicValue::Text

mod codec;
#[cfg(test)]
pub(super) mod fixture;
mod layout;
mod property;
mod table;

/// Row derive, compiled for the macro host and evaluated for the host target.
pub use ipp_schema_derive::SchemaRow;

pub use codec::{decode_row, decode_row_value, encode_row, encode_row_value};
pub use layout::{
    MAX_ROW_FIELDS, MAX_ROW_PROPERTIES, MAX_ROW_TEXT_BYTES, ROW_REGION_SPAN, RowAddress,
    RowProperty, RowPropertyHint, RowsLayout, max_row_slots, row_address, row_property_offset,
    row_region, row_region_base, row_region_relative,
};
pub use property::{RowPropertyField, RowPropertyValue, SchemaRow, check_row_text};
pub use table::{Rows, SchemaRowsField};
