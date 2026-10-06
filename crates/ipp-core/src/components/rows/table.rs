//! The `Rows<R>` table: never-reused slot identities, region-relative property
//! access and the whole-table field encoding.

use super::codec::{decode_row, encode_row, take_u32};
use super::layout::{RowsLayout, max_row_slots, row_address, row_property_offset, row_region_base};
use super::property::SchemaRow;
use crate::components::dynamic_properties::{DynamicPropertyKind, DynamicValue};
use crate::components::schema::{ContractSink, FieldError, FieldKind, FieldValue, SchemaField};
use crate::services::asset_management::AssetSource;

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

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
