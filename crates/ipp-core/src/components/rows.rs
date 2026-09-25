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
//! property is `String` or `Option<String>` declaring `#[schema(text = N)]`, its
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

use super::dynamic_properties::{DynamicPropertyKind, DynamicValue};
use super::schema::{ContractSink, FieldError, FieldKind, FieldValue, SchemaField, write_string};
use crate::services::asset_management::{AssetSource, AssetTypeId};

/// Row derive, compiled for the macro host and evaluated for the host target.
pub use ipp_schema_derive::SchemaRow;

/// Offset span of one rows region.
pub const ROW_REGION_SPAN: u32 = 0x1000_0000;

/// Maximum rows fields per component; region seven ends at the dynamic bit.
pub const MAX_ROW_FIELDS: usize = 7;

/// Largest byte bound a text row property may declare: one string field value.
pub const MAX_ROW_TEXT_BYTES: u32 = 65_536;

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
        value.validate()?;
        match value {
            DynamicValue::Asset(value) => Ok(value),
            _ => Err(FieldError::WrongType),
        }
    }

    fn asset(&self) -> Option<&AssetSource> {
        Some(self)
    }

    fn heap_bytes(&self) -> usize {
        self.uri.capacity()
    }
}

/// Text rows accept any UTF-8 string here; the derive adds the declared byte
/// bound through [`check_row_text`].
impl RowPropertyValue for String {
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
        self.capacity()
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

/// Per-slot state inside one rows field, observed by tests.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowSlotState {
    /// Never held a row in this incarnation; may receive [`Rows::insert`].
    Unallocated,
    /// Holds a row.
    Live,
    /// Held a row that was removed; never reused in this incarnation.
    Dead,
}

/// A compiled table of `R` rows keyed by never-reused slot identities.
///
/// Live rows are stored in ascending slot order, so lookups are a direct index
/// while slots stay dense from zero and a binary search otherwise. Iteration
/// visits live rows in slot order.
///
/// Equality compares [`Self::next_slot`] and the live rows only, exactly what the
/// table encoding carries; the in-memory dead-slot record is not part of the value.
#[derive(Clone, Debug)]
pub struct Rows<R> {
    rows: Vec<(u32, R)>,
    next_slot: u32,
    /// Removed slots in ascending order; never encoded.
    dead: Vec<u32>,
}

impl<R> Default for Rows<R> {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            next_slot: 0,
            dead: Vec::new(),
        }
    }
}

impl<R: PartialEq> PartialEq for Rows<R> {
    fn eq(&self, other: &Self) -> bool {
        self.next_slot == other.next_slot && self.rows == other.rows
    }
}

impl<R: SchemaRow> Rows<R> {
    /// Slots addressable by this row layout.
    pub const MAX_SLOTS: u32 = max_row_slots(R::LAYOUT.property_count());

    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// One past the highest slot allocated so far; [`Self::push`] allocates it.
    pub fn next_slot(&self) -> u32 {
        self.next_slot
    }

    /// Number of live rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether no row is live.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    fn position(&self, slot: u32) -> Option<usize> {
        if let Some((candidate, _)) = self.rows.get(slot as usize)
            && *candidate == slot
        {
            return Some(slot as usize);
        }

        self.rows
            .binary_search_by_key(&slot, |(slot, _)| *slot)
            .ok()
    }

    /// Classify a slot.
    #[cfg(test)]
    pub(crate) fn slot_state(&self, slot: u32) -> RowSlotState {
        if self.position(slot).is_some() {
            RowSlotState::Live
        } else if self.dead.binary_search(&slot).is_ok() {
            RowSlotState::Dead
        } else {
            RowSlotState::Unallocated
        }
    }

    /// Whether a slot holds a live row.
    pub fn is_live(&self, slot: u32) -> bool {
        self.position(slot).is_some()
    }

    /// Borrow a live row.
    pub fn get(&self, slot: u32) -> Option<&R> {
        self.position(slot).map(|index| &self.rows[index].1)
    }

    /// Mutably borrow a live row. Callers own value validation for direct edits.
    pub fn get_mut(&mut self, slot: u32) -> Option<&mut R> {
        self.position(slot).map(|index| &mut self.rows[index].1)
    }

    /// Append a row at [`Self::next_slot`] and return its slot.
    pub fn push(&mut self, row: R) -> Result<u32, FieldError> {
        let slot = self.next_slot;
        self.insert(slot, row)?;
        Ok(slot)
    }

    /// Place a row at any unallocated slot below [`Self::MAX_SLOTS`], in any
    /// order; slots at or above [`Self::next_slot`] advance it past `slot`. Live,
    /// dead and unaddressable slots reject the row.
    pub fn insert(&mut self, slot: u32, row: R) -> Result<(), FieldError> {
        if slot >= Self::MAX_SLOTS || self.dead.binary_search(&slot).is_ok() {
            return Err(FieldError::UnknownField);
        }

        if slot >= self.next_slot {
            self.rows.push((slot, row));
            self.next_slot = slot + 1;
            return Ok(());
        }

        let Err(index) = self.rows.binary_search_by_key(&slot, |(slot, _)| *slot) else {
            return Err(FieldError::UnknownField);
        };
        self.rows.insert(index, (slot, row));
        Ok(())
    }

    /// Mark a live slot dead and return its row. Dead slots are never reused
    /// within this incarnation.
    pub fn remove(&mut self, slot: u32) -> Option<R> {
        let index = self.position(slot)?;
        let dead = self.dead.binary_search(&slot).unwrap_err();
        self.dead.insert(dead, slot);
        Some(self.rows.remove(index).1)
    }

    /// Mark every live slot in `slots` dead in one pass and return their rows in
    /// ascending slot order. Slots that are not live (dead, unallocated,
    /// unaddressable or repeated) are ignored. For `n` live rows, `d` dead slots
    /// and `k` requested slots this costs O(n + d + k log k), where removing the
    /// same slots one at a time with [`Self::remove`] shifts the table per slot.
    pub fn remove_slots(&mut self, slots: &[u32]) -> Vec<R> {
        let mut removing = slots.to_vec();
        removing.sort_unstable();
        removing.dedup();
        let (Some(&first), Some(&last)) = (removing.first(), removing.last()) else {
            return Vec::new();
        };

        // Both sequences ascend, so one cursor classifies every row in range.
        let start = self.rows.partition_point(|(slot, _)| *slot < first);
        let end = self.rows.partition_point(|(slot, _)| *slot <= last);
        let mut next = 0;
        let removed: Vec<(u32, R)> = self
            .rows
            .extract_if(start..end, |(slot, _)| {
                while removing.get(next).is_some_and(|candidate| candidate < slot) {
                    next += 1;
                }

                removing.get(next) == Some(slot)
            })
            .collect();

        // Live slots are never dead, so the extracted slots are disjoint from the record.
        removing.clear();
        removing.extend(removed.iter().map(|(slot, _)| *slot));
        self.merge_dead(&removing);
        removed.into_iter().map(|(_, row)| row).collect()
    }

    /// Merge ascending slots disjoint from the dead record into it, back to front.
    fn merge_dead(&mut self, slots: &[u32]) {
        let mut dead = self.dead.len();
        let mut added = slots.len();
        self.dead.resize(dead + added, 0);

        let mut write = self.dead.len();
        while added > 0 {
            write -= 1;
            if dead > 0 && self.dead[dead - 1] > slots[added - 1] {
                self.dead[write] = self.dead[dead - 1];
                dead -= 1;
            } else {
                self.dead[write] = slots[added - 1];
                added -= 1;
            }
        }
    }

    /// Live rows in ascending slot order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (u32, &R)> + '_ {
        self.rows.iter().map(|(slot, row)| (*slot, row))
    }

    /// Mutable live rows in ascending slot order.
    pub fn iter_mut(&mut self) -> impl ExactSizeIterator<Item = (u32, &mut R)> + '_ {
        self.rows.iter_mut().map(|(slot, row)| (*slot, row))
    }

    /// Copy one property of a live row; `Ok(None)` is an absent optional property.
    pub fn property(&self, slot: u32, property: u32) -> Result<Option<DynamicValue>, FieldError> {
        self.get(slot)
            .ok_or(FieldError::UnknownField)?
            .property(property)
    }

    /// Replace one property of a live row.
    pub fn set_property(
        &mut self,
        slot: u32,
        property: u32,
        value: DynamicValue,
    ) -> Result<(), FieldError> {
        self.get_mut(slot)
            .ok_or(FieldError::UnknownField)?
            .set_property(property, value)
    }

    /// Clear one optional property of a live row.
    pub fn clear_property(&mut self, slot: u32, property: u32) -> Result<(), FieldError> {
        self.get_mut(slot)
            .ok_or(FieldError::UnknownField)?
            .clear_property(property)
    }

    /// Complete field offset of a property in rows field `field`.
    pub const fn offset(field: usize, slot: u32, property: u32) -> Option<u32> {
        row_property_offset(field, R::LAYOUT.property_count(), slot, property)
    }

    /// Whether a region-relative address names a property this layout can hold.
    pub fn has_row_field(relative: u32) -> bool {
        row_address(relative, R::LAYOUT.property_count()).is_some()
    }

    /// Read a region-relative property: `String` for present text, `Dynamic` for
    /// any other present value, `Unset` for an absent optional property, and an
    /// error for a dead or unallocated slot.
    pub fn row_field(&self, relative: u32) -> Result<FieldValue, FieldError> {
        let address =
            row_address(relative, R::LAYOUT.property_count()).ok_or(FieldError::UnknownField)?;
        Ok(match self.property(address.slot, address.property)? {
            Some(DynamicValue::Text(text)) => FieldValue::String(text),
            Some(value) => FieldValue::Dynamic(value),
            None => FieldValue::Unset,
        })
    }

    /// Write a region-relative property: `String` replaces text, `Dynamic`
    /// replaces any other kind and `Unset` clears an optional property. Dead and
    /// unallocated slots reject every write.
    pub fn set_row_field(&mut self, relative: u32, value: FieldValue) -> Result<(), FieldError> {
        let address =
            row_address(relative, R::LAYOUT.property_count()).ok_or(FieldError::UnknownField)?;
        let text =
            R::LAYOUT.properties[address.property as usize].kind == DynamicPropertyKind::Text;

        match value {
            FieldValue::String(value) if text => {
                self.set_property(address.slot, address.property, DynamicValue::Text(value))
            }
            FieldValue::Dynamic(value) if value.kind() != DynamicPropertyKind::Text => {
                self.set_property(address.slot, address.property, value)
            }
            FieldValue::Unset => self.clear_property(address.slot, address.property),
            _ => Err(FieldError::WrongType),
        }
    }

    /// Check a region-relative address and operation kind without an instance:
    /// text properties take `String`, other properties `Dynamic`, and optional
    /// ones `Unset`. Value kinds and text bounds are checked by the write itself.
    pub fn validate_row_field(relative: u32, kind: FieldKind) -> Result<(), FieldError> {
        let address =
            row_address(relative, R::LAYOUT.property_count()).ok_or(FieldError::UnknownField)?;
        let property = R::LAYOUT.properties[address.property as usize];
        let text = property.kind == DynamicPropertyKind::Text;
        match kind {
            FieldKind::Dynamic if !text => Ok(()),
            FieldKind::String if text => Ok(()),
            FieldKind::Unset if property.optional => Ok(()),
            _ => Err(FieldError::WrongType),
        }
    }

    /// Heap bytes retained by the table, its dead-slot record and its rows.
    pub fn retained_bytes(&self) -> usize {
        self.rows.capacity() * std::mem::size_of::<(u32, R)>()
            + self.dead.capacity() * std::mem::size_of::<u32>()
            + self
                .rows
                .iter()
                .map(|(_, row)| row.retained_bytes())
                .sum::<usize>()
    }

    /// Visit present asset properties of live rows in slot and layout order.
    pub fn visit_assets(&self, visit: &mut dyn FnMut(&AssetSource)) {
        for (_, row) in &self.rows {
            row.visit_assets(visit);
        }
    }

    /// Add every nonempty asset property to a component's resource demand; the
    /// owning component calls this from its lifecycle resource hook.
    #[cfg(any(test, feature = "gui"))]
    pub(crate) fn resource_demand(
        &self,
        demand: &mut std::collections::BTreeSet<
            crate::services::asset_management::service::AssetDemandSelection,
        >,
    ) {
        self.visit_assets(&mut |asset| {
            if !asset.uri.is_empty() {
                crate::services::asset_management::service::AssetDemandSelection::insert_into(
                    demand,
                    asset.kind,
                    &asset.uri,
                    asset.variant,
                );
            }
        });
    }

    /// Encode the table: little-endian `u32` next slot and `u32` live row count,
    /// then per live row in ascending slot order a `u32` slot, a presence mask of
    /// [`RowsLayout::mask_bytes`] bytes (bit `i % 8` of byte `i / 8` marks
    /// property `i`), and the present values in layout order. Values carry no
    /// tag: 4-byte F32/I32/U32/Bool (0 or 1), 8/12/16-byte vectors, assets as
    /// `u16` type, `u32` variant, `u32` source length and UTF-8 source, and text
    /// as `u32` byte length and UTF-8 bytes within the property's bound.
    pub fn encode(&self) -> Vec<u8> {
        let layout = R::LAYOUT;
        let mut bytes = Vec::with_capacity(8 + self.rows.len() * (4 + layout.mask_bytes()));
        bytes.extend(self.next_slot.to_le_bytes());
        bytes.extend((self.rows.len() as u32).to_le_bytes());

        for (slot, row) in &self.rows {
            bytes.extend(slot.to_le_bytes());
            encode_row(row, &mut bytes);
        }

        bytes
    }

    /// Decode and validate a complete table produced by [`Self::encode`].
    pub fn decode(mut bytes: &[u8]) -> Result<Self, FieldError> {
        let layout = R::LAYOUT;
        let next_slot = take_u32(&mut bytes)?;
        let count = take_u32(&mut bytes)? as usize;
        if next_slot > Self::MAX_SLOTS || count > bytes.len() / (4 + layout.mask_bytes()) {
            return Err(FieldError::WrongType);
        }

        // A decoded table starts a new incarnation without dead slots.
        let mut table = Self {
            rows: Vec::with_capacity(count),
            next_slot: 0,
            dead: Vec::new(),
        };
        for _ in 0..count {
            let slot = take_u32(&mut bytes)?;
            if slot >= next_slot || table.rows.last().is_some_and(|(last, _)| *last >= slot) {
                return Err(FieldError::WrongType);
            }

            let row = decode_row(&mut bytes)?;
            table.rows.push((slot, row));
        }

        if !bytes.is_empty() {
            return Err(FieldError::WrongType);
        }

        table.next_slot = next_slot;
        Ok(table)
    }
}

impl<R: SchemaRow> SchemaField for Rows<R> {
    const KIND: FieldKind = FieldKind::Rows;

    fn to_value(&self) -> FieldValue {
        FieldValue::Rows(self.encode())
    }

    fn from_value(value: FieldValue) -> Result<Self, FieldError> {
        match value {
            FieldValue::Rows(bytes) => Self::decode(&bytes),
            _ => Err(FieldError::WrongType),
        }
    }

    fn write_default(&self, sink: &mut impl ContractSink) {
        let bytes = self.encode();
        sink.write(&(bytes.len() as u32).to_le_bytes());
        sink.write(&bytes);
    }

    fn retained_bytes(&self) -> Option<usize> {
        Some(Rows::retained_bytes(self))
    }
}

/// Region dispatch used by `#[derive(SchemaComponent)]` for `#[schema(rows)]` fields.
pub trait SchemaRowsField {
    /// Row layout streamed into the target contract.
    const LAYOUT: RowsLayout;

    /// See [`Rows::has_row_field`].
    fn has_row_field(relative: u32) -> bool;

    /// See [`Rows::row_field`].
    fn row_field(&self, relative: u32) -> Result<FieldValue, FieldError>;

    /// See [`Rows::set_row_field`].
    fn set_row_field(&mut self, relative: u32, value: FieldValue) -> Result<(), FieldError>;

    /// See [`Rows::validate_row_field`].
    fn validate_row_field(relative: u32, kind: FieldKind) -> Result<(), FieldError>;

    /// See [`Rows::visit_assets`].
    fn visit_assets(&self, visit: &mut dyn FnMut(&AssetSource));

    /// Stream a rows field's region base and layout after its field kind.
    fn write_row_contract(field: usize, sink: &mut impl ContractSink) {
        sink.write(&row_region_base(field).to_le_bytes());
        Self::LAYOUT.write(sink);
    }
}

impl<R: SchemaRow> SchemaRowsField for Rows<R> {
    const LAYOUT: RowsLayout = R::LAYOUT;

    fn has_row_field(relative: u32) -> bool {
        Self::has_row_field(relative)
    }

    fn row_field(&self, relative: u32) -> Result<FieldValue, FieldError> {
        self.row_field(relative)
    }

    fn set_row_field(&mut self, relative: u32, value: FieldValue) -> Result<(), FieldError> {
        self.set_row_field(relative, value)
    }

    fn validate_row_field(relative: u32, kind: FieldKind) -> Result<(), FieldError> {
        Self::validate_row_field(relative, kind)
    }

    fn visit_assets(&self, visit: &mut dyn FnMut(&AssetSource)) {
        self.visit_assets(visit)
    }
}

/// Append one row in the table's per-row encoding: a presence mask of
/// [`RowsLayout::mask_bytes`] bytes, then the present values in layout order
/// (see [`Rows::encode`]).
pub fn encode_row<R: SchemaRow>(row: &R, bytes: &mut Vec<u8>) {
    let layout = R::LAYOUT;
    let mask_start = bytes.len();
    bytes.resize(mask_start + layout.mask_bytes(), 0);

    for index in 0..layout.property_count() {
        let Some(value) = row.property(index).expect("layout property") else {
            continue;
        };

        bytes[mask_start + index as usize / 8] |= 1 << (index % 8);
        encode_row_value(&value, bytes);
    }
}

/// Decode and validate one row written by [`encode_row`], consuming it from
/// `bytes`. Required properties must be present; undeclared mask bits,
/// non-finite values, invalid asset sources and invalid or over-long text are
/// rejected.
pub fn decode_row<R: SchemaRow>(bytes: &mut &[u8]) -> Result<R, FieldError> {
    let layout = R::LAYOUT;
    let mask = take(bytes, layout.mask_bytes())?;
    if layout.properties.len() % 8 != 0
        && mask[mask.len() - 1] >> (layout.properties.len() % 8) != 0
    {
        return Err(FieldError::WrongType);
    }

    let mut row = R::default();
    for (index, property) in layout.properties.iter().enumerate() {
        if mask[index / 8] & (1 << (index % 8)) == 0 {
            if !property.optional {
                return Err(FieldError::WrongType);
            }

            row.clear_property(index as u32)?;
        } else {
            row.set_property(index as u32, decode_row_value(property.kind, bytes)?)?;
        }
    }

    Ok(row)
}

fn take<'a>(bytes: &mut &'a [u8], length: usize) -> Result<&'a [u8], FieldError> {
    let (head, tail) = bytes
        .split_at_checked(length)
        .ok_or(FieldError::WrongType)?;
    *bytes = tail;
    Ok(head)
}

fn take_u32(bytes: &mut &[u8]) -> Result<u32, FieldError> {
    Ok(u32::from_le_bytes(
        take(bytes, 4)?.try_into().expect("four bytes"),
    ))
}

/// Append one untagged row property value in the table encoding.
pub fn encode_row_value(value: &DynamicValue, bytes: &mut Vec<u8>) {
    match value {
        DynamicValue::I32(value) => bytes.extend(value.to_le_bytes()),
        DynamicValue::U32(value) => bytes.extend(value.to_le_bytes()),
        DynamicValue::Bool(value) => bytes.extend(u32::from(*value).to_le_bytes()),
        DynamicValue::Asset(asset) => {
            bytes.extend(asset.kind.0.to_le_bytes());
            bytes.extend(asset.variant.to_le_bytes());
            bytes.extend((asset.uri.len() as u32).to_le_bytes());
            bytes.extend(asset.uri.as_bytes());
        }
        DynamicValue::Text(text) => {
            bytes.extend((text.len() as u32).to_le_bytes());
            bytes.extend(text.as_bytes());
        }
        value => {
            for lane in value.floats().expect("numeric row value") {
                bytes.extend(lane.to_le_bytes());
            }
        }
    }
}

/// Decode and validate one untagged row property value of `kind`. Text is
/// checked for UTF-8 here and against its byte bound by the row write.
pub fn decode_row_value(
    kind: DynamicPropertyKind,
    bytes: &mut &[u8],
) -> Result<DynamicValue, FieldError> {
    fn floats<const N: usize>(bytes: &mut &[u8]) -> Result<[f32; N], FieldError> {
        let data = take(bytes, N * 4)?;
        Ok(std::array::from_fn(|lane| {
            f32::from_le_bytes(data[lane * 4..lane * 4 + 4].try_into().expect("four bytes"))
        }))
    }

    let value = match kind {
        DynamicPropertyKind::F32 => DynamicValue::F32(floats::<1>(bytes)?[0]),
        DynamicPropertyKind::I32 => DynamicValue::I32(take_u32(bytes)? as i32),
        DynamicPropertyKind::U32 => DynamicValue::U32(take_u32(bytes)?),
        DynamicPropertyKind::Bool => DynamicValue::Bool(match take_u32(bytes)? {
            0 => false,
            1 => true,
            _ => return Err(FieldError::WrongType),
        }),
        DynamicPropertyKind::Vec2 => DynamicValue::Vec2(floats(bytes)?),
        DynamicPropertyKind::Vec3 => DynamicValue::Vec3(floats(bytes)?),
        DynamicPropertyKind::Vec4 => DynamicValue::Vec4(floats(bytes)?),
        DynamicPropertyKind::Asset => {
            let kind = u16::from_le_bytes(take(bytes, 2)?.try_into().expect("two bytes"));
            let variant = take_u32(bytes)?;
            let length = take_u32(bytes)? as usize;
            let uri =
                std::str::from_utf8(take(bytes, length)?).map_err(|_| FieldError::WrongType)?;
            DynamicValue::Asset(AssetSource {
                kind: AssetTypeId(kind),
                uri: uri.into(),
                variant,
            })
        }
        DynamicPropertyKind::Text => {
            let length = take_u32(bytes)? as usize;
            if length > MAX_ROW_TEXT_BYTES as usize {
                return Err(FieldError::TextTooLong);
            }

            let text =
                std::str::from_utf8(take(bytes, length)?).map_err(|_| FieldError::WrongType)?;
            DynamicValue::Text(text.into())
        }
        DynamicPropertyKind::Mat2 | DynamicPropertyKind::Mat3 | DynamicPropertyKind::Mat4 => {
            return Err(FieldError::WrongType);
        }
    };

    value.validate()?;
    Ok(value)
}

#[cfg(test)]
#[path = "rows_tests.rs"]
mod tests;
