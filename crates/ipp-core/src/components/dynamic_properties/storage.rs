use super::{DynamicPropertyKind, DynamicValue};
use crate::{
    components::schema::{FieldError, FieldValue},
    services::asset_management::{AssetSource, AssetTypeId},
};
use std::collections::{BTreeMap, BTreeSet};

/// Internal field identity for durable descriptors, distinct from Rust offsets.
pub const DYNAMIC_METADATA: u32 = 0x8000_0000;

/// Whether a field identity belongs to the reserved dynamic namespace.
pub fn is_dynamic_field(field: u32) -> bool {
    field & DYNAMIC_METADATA != 0
}

/// A stable property identity and its current component-local storage location.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DynamicPropertyDescriptor {
    /// Never-reused identity within this component incarnation.
    pub key: u32,
    /// Exact authored storage type.
    pub kind: DynamicPropertyKind,
    /// Numeric CPU byte offset; asset references use separate typed storage.
    pub offset: u32,
}

/// The sole effective values of a component's named properties.
#[derive(Debug, PartialEq)]
pub struct DynamicProperties {
    descriptors: BTreeMap<String, DynamicPropertyDescriptor>,
    /// Prepared identity lookup. This mirrors descriptors, never values, and is
    /// rebuilt from durable metadata with the component incarnation.
    descriptors_by_key: BTreeMap<u32, DynamicPropertyDescriptor>,
    buffer: Vec<u8>,
    /// Numeric bytes held by live descriptors; the rest of the buffer is gaps.
    occupied: usize,
    assets: BTreeMap<u32, AssetSource>,
    next_key: u32,
}

impl Clone for DynamicProperties {
    fn clone(&self) -> Self {
        #[cfg(test)]
        clone_count::record();
        Self {
            descriptors: self.descriptors.clone(),
            descriptors_by_key: self.descriptors_by_key.clone(),
            buffer: self.buffer.clone(),
            occupied: self.occupied,
            assets: self.assets.clone(),
            next_key: self.next_key,
        }
    }
}

/// Per-thread count of whole property-set copies, for staging cost tests.
#[cfg(test)]
pub(crate) mod clone_count {
    use std::cell::Cell;

    thread_local! {
        static CLONES: Cell<usize> = const { Cell::new(0) };
    }

    pub(super) fn record() {
        CLONES.with(|clones| clones.set(clones.get() + 1));
    }

    /// Return and reset this thread's copy count.
    pub(crate) fn take() -> usize {
        CLONES.with(|clones| clones.replace(0))
    }
}

impl Default for DynamicProperties {
    fn default() -> Self {
        Self {
            descriptors: BTreeMap::new(),
            descriptors_by_key: BTreeMap::new(),
            buffer: Vec::new(),
            occupied: 0,
            assets: BTreeMap::new(),
            next_key: DYNAMIC_METADATA + 1,
        }
    }
}

impl DynamicProperties {
    /// Names, stable identities, types and local numeric byte offsets.
    pub fn descriptors(&self) -> &BTreeMap<String, DynamicPropertyDescriptor> {
        &self.descriptors
    }

    /// Borrow the packed CPU numeric storage; renderer packing is independent.
    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    /// Copy a typed property value by its authored name.
    pub fn get(&self, name: &str) -> Option<DynamicValue> {
        self.get_descriptor(*self.descriptors.get(name)?)
    }

    /// Borrow an asset selection without copying its owned source URI.
    pub fn asset(&self, name: &str) -> Option<&AssetSource> {
        let descriptor = self.descriptors.get(name)?;
        (descriptor.kind == DynamicPropertyKind::Asset)
            .then(|| self.assets.get(&descriptor.key))
            .flatten()
    }

    /// Copy a typed property value by its validated lifetime identity.
    pub fn get_key(&self, key: u32) -> Option<DynamicValue> {
        self.get_descriptor(*self.descriptors_by_key.get(&key)?)
    }

    /// Internal prepared access; the owner invalidates this descriptor before
    /// property removal/retyping. The buffer base is read anew after growth.
    pub(crate) fn get_descriptor(
        &self,
        descriptor: DynamicPropertyDescriptor,
    ) -> Option<DynamicValue> {
        if descriptor.kind == DynamicPropertyKind::Asset {
            return self
                .assets
                .get(&descriptor.key)
                .cloned()
                .map(DynamicValue::Asset);
        }
        let start = descriptor.offset as usize;
        if crate::allocation_optimizations_enabled() {
            let data = self.buffer.get(start..start + descriptor.kind.byte_len())?;
            let mut bytes = [0u8; 65];
            bytes[0] = descriptor.kind as u8;
            bytes[1..1 + data.len()].copy_from_slice(data);
            return DynamicValue::decode(&bytes[..1 + data.len()]).ok();
        }
        let mut bytes = vec![descriptor.kind as u8];
        bytes.extend_from_slice(self.buffer.get(start..start + descriptor.kind.byte_len())?);
        DynamicValue::decode(&bytes).ok()
    }

    /// Borrow the asset selection behind a prepared asset descriptor.
    #[cfg(feature = "gui")]
    pub(crate) fn descriptor_asset(
        &self,
        descriptor: DynamicPropertyDescriptor,
    ) -> Option<&AssetSource> {
        (descriptor.kind == DynamicPropertyKind::Asset)
            .then(|| self.assets.get(&descriptor.key))
            .flatten()
    }

    /// Names starting with `prefix`, in name order, with the remainder of
    /// each name after the prefix: one ordered seek instead of a lookup
    /// per candidate name.
    #[cfg(feature = "gui")]
    pub(crate) fn with_prefix<'a>(
        &'a self,
        prefix: &'a str,
    ) -> impl Iterator<Item = (&'a str, DynamicPropertyDescriptor)> + 'a {
        self.named_with_prefix(prefix)
            .map(move |(name, descriptor)| (&name[prefix.len()..], descriptor))
    }

    /// Full names starting with `prefix`, in name order.
    #[cfg(feature = "gui")]
    pub(crate) fn named_with_prefix<'a>(
        &'a self,
        prefix: &'a str,
    ) -> impl Iterator<Item = (&'a str, DynamicPropertyDescriptor)> + 'a {
        self.descriptors
            .range::<str, _>((
                std::ops::Bound::Included(prefix),
                std::ops::Bound::Unbounded,
            ))
            .take_while(move |(name, _)| name.starts_with(prefix))
            .map(|(name, descriptor)| (name.as_str(), *descriptor))
    }

    /// Resolve a name once for a prepared property binding.
    pub fn key(&self, name: &str) -> Option<u32> {
        self.descriptors.get(name).map(|descriptor| descriptor.key)
    }

    /// Stored representation of one identity, compared exactly as whole-value
    /// equality compares it (numeric bytes, not float equality).
    pub(crate) fn stored_value(
        &self,
        key: u32,
    ) -> Option<(DynamicPropertyKind, Vec<u8>, Option<AssetSource>)> {
        let descriptor = self.descriptors_by_key.get(&key)?;
        let start = descriptor.offset as usize;
        Some((
            descriptor.kind,
            self.buffer
                .get(start..start + descriptor.kind.byte_len())?
                .to_vec(),
            self.assets.get(&key).cloned(),
        ))
    }

    /// Resolve one live identity to its prepared descriptor without scanning names.
    pub(crate) fn descriptor(&self, key: u32) -> Option<DynamicPropertyDescriptor> {
        self.descriptors_by_key.get(&key).copied()
    }

    /// Accept ASCII identifiers suitable for generated backend parameter names.
    pub fn validate_name(name: &str) -> Result<(), FieldError> {
        let mut chars = name.bytes();
        if !chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
            || !chars.all(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            return Err(FieldError::UnknownField);
        }
        Ok(())
    }

    /// Define or update an authored property. Retyping creates a fresh identity.
    pub fn set(&mut self, name: &str, value: DynamicValue) -> Result<u32, FieldError> {
        Self::validate_name(name)?;
        value.validate()?;
        if let Some(descriptor) = self.descriptors.get(name)
            && descriptor.kind == value.kind()
        {
            let key = descriptor.key;
            self.set_key(key, value)?;
            return Ok(key);
        }
        let key = self.next_key;
        let next_key = key
            .checked_add(1)
            .filter(|v| *v != u32::MAX)
            .ok_or(FieldError::UnknownField)?;
        self.remove(name);
        let offset = self.allocate(value.kind().byte_len())?;
        let descriptor = DynamicPropertyDescriptor {
            key,
            kind: value.kind(),
            offset,
        };
        self.descriptors.insert(name.into(), descriptor);
        self.descriptors_by_key.insert(key, descriptor);
        self.occupied += descriptor.kind.byte_len();
        self.next_key = next_key;
        self.set_key(key, value)?;
        Ok(key)
    }

    /// Update an existing identity without allowing a type change.
    pub fn set_key(&mut self, key: u32, value: DynamicValue) -> Result<(), FieldError> {
        value.validate()?;
        let descriptor = self
            .descriptors_by_key
            .get(&key)
            .ok_or(FieldError::UnknownField)?;
        if descriptor.kind != value.kind() {
            return Err(FieldError::WrongType);
        }
        self.set_descriptor(*descriptor, value);
        Ok(())
    }

    /// Prepared typed write after the caller has established the value's range
    /// and descriptor lifetime; storage growth never invalidates its byte offset.
    pub(crate) fn set_descriptor(
        &mut self,
        descriptor: DynamicPropertyDescriptor,
        value: DynamicValue,
    ) {
        if let DynamicValue::Asset(asset) = value {
            self.assets.insert(descriptor.key, asset);
        } else if crate::allocation_optimizations_enabled() {
            let start = descriptor.offset as usize;
            let output = &mut self.buffer[start..start + descriptor.kind.byte_len()];
            if let Some(lanes) = value.floats() {
                for (lane, bytes) in lanes.iter().zip(output.as_chunks_mut::<4>().0.iter_mut()) {
                    bytes.copy_from_slice(&lane.to_le_bytes());
                }
            } else {
                let bytes = match value {
                    DynamicValue::I32(value) => value.to_le_bytes(),
                    DynamicValue::U32(value) => value.to_le_bytes(),
                    DynamicValue::Bool(value) => u32::from(value).to_le_bytes(),
                    _ => unreachable!("numeric value checked above"),
                };
                output.copy_from_slice(&bytes);
            }
        } else {
            let bytes = value.encode();
            let start = descriptor.offset as usize;
            self.buffer[start..start + descriptor.kind.byte_len()].copy_from_slice(&bytes[1..]);
        }
    }

    /// Remove a declaration and invalidate its key before its bytes can be reused.
    pub fn remove(&mut self, name: &str) -> Option<u32> {
        let descriptor = self.descriptors.remove(name)?;
        self.descriptors_by_key.remove(&descriptor.key);
        self.occupied -= descriptor.kind.byte_len();
        self.assets.remove(&descriptor.key);
        let start = descriptor.offset as usize;
        self.buffer[start..start + descriptor.kind.byte_len()].fill(0);
        let end = self
            .descriptors
            .values()
            .map(|d| d.offset as usize + d.kind.byte_len())
            .max()
            .unwrap_or(0);
        self.buffer.truncate(end);
        Some(descriptor.key)
    }

    /// Reserve `size` bytes at the first offset where they fit between
    /// existing properties, else at the buffer end. Gaps exist only after
    /// removals: when fewer free bytes remain than `size`, the first fit is
    /// the end, found from the retained occupied byte count without visiting
    /// descriptors. Zero-sized kinds occupy no bytes and use offset zero.
    fn allocate(&mut self, size: usize) -> Result<u32, FieldError> {
        if size == 0 {
            return Ok(0);
        }

        let mut offset = self.buffer.len();
        if self.buffer.len().saturating_sub(self.occupied) >= size {
            let mut spans: Vec<_> = self
                .descriptors_by_key
                .values()
                .filter(|d| d.kind.byte_len() > 0)
                .map(|d| (d.offset as usize, d.kind.byte_len()))
                .collect();
            spans.sort_unstable();
            offset = 0;
            for (start, length) in spans {
                if start >= offset + size {
                    break;
                }
                offset = start + length;
            }
        }

        let end = offset
            .checked_add(size)
            .filter(|n| *n <= u32::MAX as usize)
            .ok_or(FieldError::UnknownField)?;
        if end > self.buffer.len() {
            self.buffer.resize(end, 0);
        }
        Ok(offset as u32)
    }

    /// Capture durable descriptors followed by typed property values.
    pub fn fields(&self) -> Vec<(u32, FieldValue)> {
        let mut fields = vec![(DYNAMIC_METADATA, FieldValue::Bytes(self.metadata()))];
        fields.extend(self.descriptors.values().map(|d| {
            (
                d.key,
                FieldValue::Dynamic(self.get_key(d.key).expect("live property")),
            )
        }));
        fields
    }

    /// Read one property identity without serializing metadata or unrelated values.
    pub fn field(&self, key: u32) -> Result<FieldValue, FieldError> {
        if key == DYNAMIC_METADATA {
            return Ok(FieldValue::Bytes(self.metadata()));
        }

        self.get_key(key)
            .map(FieldValue::Dynamic)
            .ok_or(FieldError::UnknownField)
    }

    /// Restore metadata or write an existing validated property identity.
    pub fn set_field(&mut self, key: u32, value: FieldValue) -> Result<(), FieldError> {
        match (key, value) {
            (DYNAMIC_METADATA, FieldValue::Bytes(bytes)) => {
                *self = Self::from_metadata(&bytes)?;
                Ok(())
            }
            (_, FieldValue::Dynamic(value)) => self.set_key(key, value),
            _ => Err(FieldError::WrongType),
        }
    }

    /// Estimate owned buffer, descriptor and asset string allocations.
    pub fn retained_bytes(&self) -> usize {
        self.buffer.capacity()
            + self
                .descriptors
                .keys()
                .map(|name| name.capacity() + std::mem::size_of::<DynamicPropertyDescriptor>())
                .sum::<usize>()
            + self
                .assets
                .values()
                .map(|t| t.uri.capacity())
                .sum::<usize>()
    }

    pub(crate) fn resource_demand(
        &self,
        demand: &mut BTreeSet<crate::services::asset_management::service::AssetDemandSelection>,
    ) {
        for asset in self.assets.values().filter(|t| !t.uri.is_empty()) {
            demand.insert(
                crate::services::asset_management::service::AssetDemandSelection::new(
                    asset.kind,
                    &asset.uri,
                    asset.variant,
                ),
            );
        }
    }

    fn metadata(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(self.next_key.to_le_bytes());
        bytes.extend((self.descriptors.len() as u32).to_le_bytes());
        for (name, descriptor) in &self.descriptors {
            bytes.extend((name.len() as u32).to_le_bytes());
            bytes.extend(name.as_bytes());
            bytes.extend(descriptor.key.to_le_bytes());
            bytes.push(descriptor.kind as u8);
        }
        bytes
    }

    fn from_metadata(mut bytes: &[u8]) -> Result<Self, FieldError> {
        fn take<'a>(bytes: &mut &'a [u8], n: usize) -> Result<&'a [u8], FieldError> {
            let part = bytes.get(..n).ok_or(FieldError::WrongType)?;
            *bytes = &bytes[n..];
            Ok(part)
        }
        fn u32(bytes: &mut &[u8]) -> Result<u32, FieldError> {
            Ok(u32::from_le_bytes(take(bytes, 4)?.try_into().unwrap()))
        }
        let next_key = u32(&mut bytes)?;
        if next_key <= DYNAMIC_METADATA || next_key == u32::MAX {
            return Err(FieldError::WrongType);
        }
        let count = u32(&mut bytes)?;
        if count as usize > bytes.len() / 10 {
            return Err(FieldError::WrongType);
        }
        let mut value = Self::default();
        let mut keys = BTreeSet::new();
        for _ in 0..count {
            let length = u32(&mut bytes)? as usize;
            let name = std::str::from_utf8(take(&mut bytes, length)?)
                .map_err(|_| FieldError::WrongType)?;
            Self::validate_name(name)?;
            let key = u32(&mut bytes)?;
            let kind = DynamicPropertyKind::from_tag(take(&mut bytes, 1)?[0])?;
            if key <= DYNAMIC_METADATA
                || key >= next_key
                || !keys.insert(key)
                || value.descriptors.contains_key(name)
            {
                return Err(FieldError::WrongType);
            }
            let offset = value.buffer.len();
            value.buffer.resize(offset + kind.byte_len(), 0);
            if kind == DynamicPropertyKind::Asset {
                value.assets.insert(
                    key,
                    AssetSource {
                        kind: AssetTypeId(0),
                        uri: String::new(),
                        variant: 0,
                    },
                );
            }
            value.descriptors.insert(
                name.into(),
                DynamicPropertyDescriptor {
                    key,
                    kind,
                    offset: offset as u32,
                },
            );
            value.descriptors_by_key.insert(
                key,
                DynamicPropertyDescriptor {
                    key,
                    kind,
                    offset: offset as u32,
                },
            );
        }
        if !bytes.is_empty() {
            return Err(FieldError::WrongType);
        }
        value.occupied = value.buffer.len();
        value.next_key = next_key;
        Ok(value)
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
