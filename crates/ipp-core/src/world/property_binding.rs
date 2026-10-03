//! Compiled property access shared by constraints and animation.
//!
//! Bindings retain occupied typed cells, never pointers into dynamic buffers or
//! row tables. Owners revoke every copy before the component incarnation or
//! descriptor departs. Reads and writes borrow the owning World's storage in
//! its sequential phase; scratch values are temporary samples, not stored state.

use super::component_binding::ComponentBinding;
use crate::components::dynamic_properties::{DynamicPropertyDescriptor, is_dynamic_field};
use crate::components::registry::{ComponentCellVisitor, ComponentStorage};
use crate::components::schema::{ComponentLifecycle, FieldValue, SchemaComponent};
use crate::{DynamicPropertyKind, DynamicValue, EntityId, ErrorReason};
use std::ptr::NonNull;

type NumericPatch = [(((EntityId, u16), u32), FieldValue)];

type Read = fn(ComponentBinding<()>, &ComponentStorage, PropertyLocation) -> Option<DynamicValue>;
type Validate =
    fn(ComponentBinding<()>, &ComponentStorage, u32, FieldValue) -> Result<(), ErrorReason>;
type Write = fn(ComponentBinding<()>, &mut ComponentStorage, PropertyLocation, FieldValue);

#[derive(Clone, Copy, Debug)]
enum PropertyLocation {
    Field(u32),
    Dynamic(DynamicPropertyDescriptor),
}

#[derive(Clone, Copy, Debug)]
enum PropertyEncoding {
    F32,
    U32,
    Bool,
    Text,
    Dynamic,
}

impl PropertyEncoding {
    fn value(self, value: DynamicValue) -> FieldValue {
        match (self, value) {
            (Self::F32, DynamicValue::F32(value)) => FieldValue::F32(value),
            (Self::U32, DynamicValue::U32(value)) => FieldValue::U32(value),
            (Self::Bool, DynamicValue::Bool(value)) => FieldValue::Bool(value),
            (Self::Text, DynamicValue::Text(value)) => FieldValue::String(value),
            (Self::Dynamic, value) => FieldValue::Dynamic(value),
            _ => unreachable!("prepared exact property type"),
        }
    }
}

/// One exact property, resolved once from its schema address.
#[derive(Clone, Copy, Debug)]
pub(in crate::world) struct PropertyBinding {
    cell: ComponentBinding<()>,
    location: PropertyLocation,
    encoding: PropertyEncoding,
    kind: DynamicPropertyKind,
    read: Read,
    validate: Option<Validate>,
    range: Option<super::numeric_properties::NumericPropertyRange>,
    write: Write,
}

impl PropertyBinding {
    /// # Safety
    /// All copies must be dropped before this component incarnation or property
    /// identity departs. Access must borrow this storage; fixed numeric writes
    /// must not change resource, relationship or storage ownership.
    pub(in crate::world) unsafe fn bind(
        storage: &ComponentStorage,
        entity: EntityId,
        component: u16,
        offset: u32,
    ) -> Option<Self> {
        storage.visit_cell(
            component,
            entity.index() as usize,
            PropertyBinder {
                component,
                offset,
            },
        )?
    }

    pub(in crate::world) fn kind(self) -> DynamicPropertyKind {
        self.kind
    }

    pub(in crate::world) fn writable(self) -> bool {
        self.validate.is_some()
    }

    /// Check descriptor layout against a staged replacement without reading
    /// through the compiled cell. Stable keys do not imply stable byte offsets
    /// when durable metadata is decoded again.
    pub(in crate::world) fn retains_layout(self, value: &crate::ComponentValue) -> bool {
        match self.location {
            PropertyLocation::Dynamic(descriptor) => {
                value
                    .dynamic_properties()
                    .and_then(|properties| properties.descriptor(descriptor.key))
                    == Some(descriptor)
            }
            PropertyLocation::Field(_) => true,
        }
    }

    pub(in crate::world) fn read(self, storage: &ComponentStorage) -> Option<DynamicValue> {
        (self.read)(self.cell, storage, self.location)
    }

    /// Check the complete candidate before notifying absolute/numeric consumers.
    pub(in crate::world) fn validate(
        self,
        storage: &ComponentStorage,
        value: &DynamicValue,
    ) -> Result<(), ErrorReason> {
        if value.kind() != self.kind {
            return Err(ErrorReason::InvalidField);
        }
        value.validate().map_err(|_| ErrorReason::InvalidValue)?;
        let validate = self.validate.ok_or(ErrorReason::InvalidField)?;
        if let Some(range) = self.range {
            return match value {
                DynamicValue::F32(value) if range.contains(*value) => Ok(()),
                _ => Err(ErrorReason::InvalidValue),
            };
        }
        if let PropertyLocation::Field(offset) = self.location {
            validate(
                self.cell,
                storage,
                offset,
                self.encoding.value(value.clone()),
            )?;
        }
        Ok(())
    }

    /// Publish an already validated candidate in the same exclusive phase.
    pub(in crate::world) fn write_validated(
        self,
        storage: &mut ComponentStorage,
        value: DynamicValue,
    ) {
        (self.write)(
            self.cell,
            storage,
            self.location,
            self.encoding.value(value),
        );
    }
}

struct PropertyBinder {
    component: u16,
    offset: u32,
}

impl ComponentCellVisitor for PropertyBinder {
    type Output = Option<PropertyBinding>;

    fn visit<C>(self, cell: NonNull<C>) -> Self::Output
    where
        C: SchemaComponent + ComponentLifecycle + 'static,
    {
        // SAFETY: visit_cell supplied this occupied C cell. The owner's bind
        // contract revokes it before incarnation/descriptor reuse. Typed function
        // pointers restore exactly C, and every access borrows its owning storage.
        let value = unsafe { cell.as_ref() };
        let (location, encoding, sample) = if is_dynamic_field(self.offset) {
            let properties = value.dynamic_properties()?;
            let descriptor = properties.descriptor(self.offset)?;
            (
                PropertyLocation::Dynamic(descriptor),
                PropertyEncoding::Dynamic,
                properties.get_descriptor(descriptor)?,
            )
        } else {
            let (encoding, sample) = match value.field(self.offset).ok()? {
                FieldValue::F32(value) => (PropertyEncoding::F32, DynamicValue::F32(value)),
                FieldValue::U32(value) => (PropertyEncoding::U32, DynamicValue::U32(value)),
                FieldValue::Bool(value) => (PropertyEncoding::Bool, DynamicValue::Bool(value)),
                FieldValue::String(value) => (PropertyEncoding::Text, DynamicValue::Text(value)),
                FieldValue::Dynamic(value) => (PropertyEncoding::Dynamic, value),
                _ => return None,
            };
            (PropertyLocation::Field(self.offset), encoding, sample)
        };
        if sample.kind() == DynamicPropertyKind::Asset {
            return None;
        }
        let range = super::numeric_properties::fixed_range(self.component, self.offset);
        let writable = (C::supports_numeric_property(self.offset) || range.is_some())
            && sample.kind() != DynamicPropertyKind::Text;
        Some(PropertyBinding {
            // SAFETY: Erase only this occupied C cell; the function pointers
            // below were monomorphized for C and restore its type on each access.
            cell: unsafe { ComponentBinding::new(cell.cast()) },
            location,
            encoding,
            kind: sample.kind(),
            read: read::<C>,
            validate: writable.then_some(validate::<C>),
            range,
            write: write::<C>,
        })
    }
}

fn read<C: SchemaComponent + ComponentLifecycle>(
    cell: ComponentBinding<()>,
    storage: &ComponentStorage,
    location: PropertyLocation,
) -> Option<DynamicValue> {
    // SAFETY: PropertyBinder paired this cell with functions for its exact C.
    let value = unsafe { cell.cast::<C>() }.get(storage);
    match location {
        PropertyLocation::Dynamic(descriptor) => {
            value.dynamic_properties()?.get_descriptor(descriptor)
        }
        PropertyLocation::Field(offset) => match value.field(offset).ok()? {
            FieldValue::F32(value) => Some(DynamicValue::F32(value)),
            FieldValue::U32(value) => Some(DynamicValue::U32(value)),
            FieldValue::Bool(value) => Some(DynamicValue::Bool(value)),
            FieldValue::String(value) => Some(DynamicValue::Text(value)),
            FieldValue::Dynamic(value) => Some(value),
            _ => None,
        },
    }
}

fn validate<C: SchemaComponent + ComponentLifecycle>(
    cell: ComponentBinding<()>,
    storage: &ComponentStorage,
    offset: u32,
    value: FieldValue,
) -> Result<(), ErrorReason> {
    // SAFETY: PropertyBinder paired this cell with functions for its exact C.
    unsafe { cell.cast::<C>() }
        .get(storage)
        .validate_numeric_properties(&[(offset, value)])
}

fn write<C: SchemaComponent + ComponentLifecycle>(
    cell: ComponentBinding<()>,
    storage: &mut ComponentStorage,
    location: PropertyLocation,
    value: FieldValue,
) {
    // SAFETY: PropertyBinder paired this cell with functions for its exact C.
    // The exclusive storage borrow prevents aliases during this numeric write.
    let component = unsafe { cell.cast::<C>() }.get_mut(storage);
    match location {
        PropertyLocation::Dynamic(descriptor) => {
            let FieldValue::Dynamic(value) = value else {
                unreachable!()
            };
            component
                .dynamic_properties_mut()
                .expect("bound dynamic store")
                .set_descriptor(descriptor, value);
        }
        PropertyLocation::Field(offset) => {
            component
                .set_field(offset, value)
                .expect("validated numeric field");
        }
    }
}

/// Prepared destinations for ordinary animation of a registered dynamic
/// component. Each destination retains its typed cell and exact descriptor;
/// patch publication never resolves names, descriptors or registry fields.
#[derive(Clone, Debug)]
pub(in crate::world) struct NumericPropertyComponent {
    properties: std::sync::Arc<[(u32, PropertyBinding)]>,
}

impl NumericPropertyComponent {
    /// # Safety
    /// Revoke before any bound property or incarnation departs; access only
    /// with the owning storage. The animation lifecycle owns this barrier.
    pub(in crate::world) unsafe fn bind(
        storage: &ComponentStorage,
        entity: EntityId,
        component: u16,
        offsets: &[u32],
    ) -> Option<Self> {
        let mut properties = Vec::with_capacity(offsets.len());
        for &offset in offsets {
            // SAFETY: The caller's barrier covers each selected property and
            // its occupied stable component cell; buffers are never retained.
            let binding = unsafe { PropertyBinding::bind(storage, entity, component, offset) }?;
            if !binding.writable() {
                return None;
            }
            properties.push((offset, binding));
        }
        properties.sort_unstable_by_key(|(offset, _)| *offset);
        properties.dedup_by_key(|(offset, _)| *offset);
        Some(Self {
            properties: properties.into(),
        })
    }

    pub(in crate::world) fn write(
        &self,
        storage: &mut ComponentStorage,
        key: (EntityId, u16),
        fields: &NumericPatch,
    ) -> Result<(), ErrorReason> {
        // Validate the complete numeric patch before any write. Each prepared
        // destination owns independent numeric validation, just like a frozen
        // transition output. No resource or relationship fields bind here.
        for ((target, offset), value) in fields.iter().filter(|((target, _), _)| *target == key) {
            let _ = target;
            let binding = self.property(*offset).ok_or(ErrorReason::InvalidField)?;
            binding.validate(
                storage,
                &numeric_value(value).ok_or(ErrorReason::InvalidField)?,
            )?;
        }
        for ((_, offset), value) in fields.iter().filter(|((target, _), _)| *target == key) {
            self.property(*offset)
                .expect("validated patch destination")
                .write_validated(
                    storage,
                    numeric_value(value).expect("validated numeric value"),
                );
        }
        Ok(())
    }

    fn property(&self, offset: u32) -> Option<PropertyBinding> {
        self.properties
            .binary_search_by_key(&offset, |(offset, _)| *offset)
            .ok()
            .map(|index| self.properties[index].1)
    }
}

fn numeric_value(value: &FieldValue) -> Option<DynamicValue> {
    match value {
        FieldValue::F32(value) => Some(DynamicValue::F32(*value)),
        FieldValue::U32(value) => Some(DynamicValue::U32(*value)),
        FieldValue::Bool(value) => Some(DynamicValue::Bool(*value)),
        FieldValue::Dynamic(value) if value.kind() != DynamicPropertyKind::Asset => {
            Some(value.clone())
        }
        _ => None,
    }
}
