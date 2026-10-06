//! Heap retention accounting for queued commands and entity metadata.

use super::{Command, EntityMetadata, EntityRef, FieldValue, FieldWrite};

impl Command {
    /// Heap bytes this command retains beyond its inline slot, or `None` when a
    /// payload cannot state a bound.
    ///
    /// Every owned allocation counts at its capacity, so queued batches and
    /// Host-buffered batch pages charge what they actually keep alive. The match
    /// is exhaustive: a new command variant must declare its retention here.
    pub fn retained_heap_bytes(&self) -> Option<usize> {
        match self {
            Self::DetachWorldAttachmentReceipt {
                ..
            } => Some(0),
            Self::Delete {
                entity,
            }
            | Self::DeleteSubtree {
                root: entity,
            }
            | Self::RemoveComponent {
                entity,
                ..
            } => Some(entity_ref_heap_bytes(entity)),
            Self::PlaceEntity {
                entity,
                placement,
            } => entity_ref_heap_bytes(entity).checked_add(placement_heap_bytes(placement)),
            Self::DetachWorldAttachmentIf {
                expected,
            } => Some(expected.retained_bytes()),
            Self::Create {
                metadata,
                ..
            } => metadata_bytes(metadata),
            Self::SetMetadata {
                entity,
                metadata,
            } => metadata_bytes(metadata)?.checked_add(entity_ref_heap_bytes(entity)),
            Self::InsertComponentValue {
                entity,
                value,
            } => std::mem::size_of::<crate::ComponentValue>()
                .checked_add(value.retained_bytes()?)?
                .checked_add(entity_ref_heap_bytes(entity)),
            Self::InsertComponent {
                entity,
                fields,
                ..
            } => field_writes_bytes(fields)?.checked_add(entity_ref_heap_bytes(entity)),
            Self::SetField {
                entity,
                field,
                ..
            } => field_value_heap_bytes(&field.value).checked_add(entity_ref_heap_bytes(entity)),
            Self::SetFieldIf {
                entity,
                field,
                expected,
                ..
            } => std::mem::size_of::<FieldValue>()
                .checked_add(field_value_heap_bytes(expected))?
                .checked_add(field_value_heap_bytes(&field.value))?
                .checked_add(entity_ref_heap_bytes(entity)),
            Self::SetDynamicProperty {
                entity,
                name,
                value,
                ..
            } => name
                .capacity()
                .checked_add(dynamic_value_heap_bytes(value))?
                .checked_add(entity_ref_heap_bytes(entity)),
            Self::RemoveDynamicProperty {
                entity,
                name,
                ..
            } => name.capacity().checked_add(entity_ref_heap_bytes(entity)),
            Self::GuiAction {
                target,
                action,
            } => {
                let text = match action {
                    crate::systems::gui::local::GuiLocalAction::SetText(text) => text.len(),
                    _ => 0,
                };
                text.checked_add(entity_ref_heap_bytes(&target.entity))
            }
        }
    }
}

fn entity_ref_heap_bytes(reference: &EntityRef) -> usize {
    match reference {
        EntityRef::Symbol(symbol) => symbol.len(),
        EntityRef::Handle(_) | EntityRef::Alias(_) => 0,
    }
}

fn placement_heap_bytes(placement: &crate::EntityPlacementRef) -> usize {
    placement
        .parent
        .iter()
        .chain(&placement.before)
        .map(entity_ref_heap_bytes)
        .sum()
}

fn field_writes_bytes(fields: &Vec<FieldWrite>) -> Option<usize> {
    fields.iter().try_fold(
        fields
            .capacity()
            .checked_mul(std::mem::size_of::<FieldWrite>())?,
        |bytes, field| bytes.checked_add(field_value_heap_bytes(&field.value)),
    )
}

fn field_value_heap_bytes(value: &FieldValue) -> usize {
    match value {
        FieldValue::String(value) => value.len(),
        FieldValue::Bytes(value) | FieldValue::Rows(value) => value.capacity(),
        FieldValue::Dynamic(value) => dynamic_value_heap_bytes(value),
        FieldValue::UnresolvedWorld(_)
        | FieldValue::UnresolvedOutput(_)
        | FieldValue::World(_)
        | FieldValue::Output(_)
        | FieldValue::F32(_)
        | FieldValue::U32(_)
        | FieldValue::U64(_)
        | FieldValue::Bool(_)
        | FieldValue::Unset => 0,
        FieldValue::Entity(reference) => entity_ref_heap_bytes(reference),
    }
}

fn dynamic_value_heap_bytes(value: &crate::DynamicValue) -> usize {
    match value {
        crate::DynamicValue::Text(text) => text.len(),
        crate::DynamicValue::Asset(source) => source.uri.len(),
        crate::DynamicValue::F32(_)
        | crate::DynamicValue::I32(_)
        | crate::DynamicValue::U32(_)
        | crate::DynamicValue::Bool(_)
        | crate::DynamicValue::Vec2(_)
        | crate::DynamicValue::Vec3(_)
        | crate::DynamicValue::Vec4(_)
        | crate::DynamicValue::Mat2(_)
        | crate::DynamicValue::Mat3(_)
        | crate::DynamicValue::Mat4(_) => 0,
    }
}

/// Heap bytes retained by entity metadata, with a fixed per-class allowance.
pub(crate) fn metadata_bytes(metadata: &EntityMetadata) -> Option<usize> {
    let mut bytes = metadata
        .classes
        .capacity()
        .checked_mul(std::mem::size_of::<String>() + 128)?;
    if let Some(symbol) = &metadata.symbolic_id {
        bytes = bytes.checked_add(symbol.capacity())?;
    }
    for class in &metadata.classes {
        bytes = bytes.checked_add(class.capacity())?;
    }
    Some(bytes)
}
