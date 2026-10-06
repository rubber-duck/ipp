//! Rows region addressing in the component field-offset space and the compiled
//! per-row property layout.

use crate::components::dynamic_properties::DynamicPropertyKind;
use crate::components::schema::{ContractSink, write_string};

/// Offset span of one rows region.
pub const ROW_REGION_SPAN: u32 = 0x1000_0000;

/// Maximum rows fields per component; region seven ends at the dynamic bit.
pub const MAX_ROW_FIELDS: usize = 7;

/// Largest byte bound a text row property may declare: one string field value.
pub const MAX_ROW_TEXT_BYTES: u32 = 65_536;

/// Properties one row type declares; the row derive rejects larger types at compile time.
///
/// One presence-mask byte covers eight properties, so 256 properties cost 32 mask bytes per
/// row, and the count fits the contract's 16-bit layout size. Maintained rows use a handful.
pub const MAX_ROW_PROPERTIES: usize = 256;

/// First property address of rows field `field` (0-based among rows fields).
pub const fn row_region_base(field: usize) -> u32 {
    assert!(
        field < MAX_ROW_FIELDS,
        "rows field index exceeds the region count"
    );
    ROW_REGION_SPAN * (field as u32 + 1)
}

/// Region-relative address when `offset` lies inside rows field `field`'s region.
pub const fn row_region_relative(offset: u32, field: usize) -> Option<u32> {
    if field >= MAX_ROW_FIELDS {
        return None;
    }

    let base = row_region_base(field);
    if offset >= base && offset - base < ROW_REGION_SPAN {
        Some(offset - base)
    } else {
        None
    }
}

/// Rows field index and region-relative address of any offset inside a rows region.
pub const fn row_region(offset: u32) -> Option<(usize, u32)> {
    let region = (offset / ROW_REGION_SPAN) as usize;
    if region == 0 || region > MAX_ROW_FIELDS {
        return None;
    }

    Some((region - 1, offset % ROW_REGION_SPAN))
}

/// Decoded row property address inside one region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowAddress {
    /// Row slot identity.
    pub slot: u32,
    /// Property index in layout order.
    pub property: u32,
}

/// Decode a region-relative address for a layout with `property_count` properties.
/// Returns `None` for a slot the region cannot address completely.
pub const fn row_address(relative: u32, property_count: u32) -> Option<RowAddress> {
    if property_count == 0 || relative >= ROW_REGION_SPAN {
        return None;
    }

    let slot = relative / property_count;
    if slot >= max_row_slots(property_count) {
        return None;
    }

    Some(RowAddress {
        slot,
        property: relative % property_count,
    })
}

/// Complete field offset of one row property, or `None` when out of range.
pub const fn row_property_offset(
    field: usize,
    property_count: u32,
    slot: u32,
    property: u32,
) -> Option<u32> {
    if field >= MAX_ROW_FIELDS
        || property >= property_count
        || slot >= max_row_slots(property_count)
    {
        return None;
    }

    Some(row_region_base(field) + slot * property_count + property)
}

/// Number of slots one region addresses for a layout: slot identities are below this.
pub const fn max_row_slots(property_count: u32) -> u32 {
    match ROW_REGION_SPAN.checked_div(property_count) {
        Some(slots) => slots,
        None => 0,
    }
}

/// Interpretation hint carried by the layout; it does not change storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RowPropertyHint {
    /// Ordinary value.
    None = 0,
    /// A Vec4 unit quaternion, interpolated as a rotation.
    Rotation = 1,
}

/// One property of a compiled row layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowProperty {
    /// Rust field name, used by generated clients.
    pub name: &'static str,
    /// Exact storage type.
    pub kind: DynamicPropertyKind,
    /// Whether the property may be absent (`Option<T>`).
    pub optional: bool,
    /// Interpretation hint.
    pub hint: RowPropertyHint,
    /// UTF-8 byte bound of a [`DynamicPropertyKind::Text`] property; zero for
    /// every other kind.
    pub max_bytes: u32,
}

/// Ordered property layout of one row type; the order defines property indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowsLayout {
    /// Properties in declaration order.
    pub properties: &'static [RowProperty],
}

impl RowsLayout {
    /// Number of properties per row.
    pub const fn property_count(&self) -> u32 {
        self.properties.len() as u32
    }

    /// Bytes in one row's presence mask.
    pub const fn mask_bytes(&self) -> usize {
        self.properties.len().div_ceil(8)
    }

    /// Stream the property count and, per property, name, kind tag, optional
    /// flag and hint, followed by a `u32` byte bound for text properties only.
    pub fn write(&self, sink: &mut impl ContractSink) {
        sink.write(&(self.properties.len() as u16).to_le_bytes());
        for property in self.properties {
            write_string(sink, property.name);
            sink.write(&[
                property.kind as u8,
                u8::from(property.optional),
                property.hint as u8,
            ]);

            if property.kind == DynamicPropertyKind::Text {
                sink.write(&property.max_bytes.to_le_bytes());
            }
        }
    }
}
