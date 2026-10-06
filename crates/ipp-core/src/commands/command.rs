//! Ordered World commands and the entity, field and metadata operands they carry.

use crate::EntityId;

/// A permanent handle, an entity named by an earlier command of this logical
/// batch, or a live entity named by its symbolic identifier.
///
/// Aliases resolve in command order at each command's own mutation boundary and
/// behave exactly like the handle they name; a name that no earlier successful
/// command of the batch defined rejects with
/// [`ErrorReason::UnknownAlias`](crate::ErrorReason::UnknownAlias). A symbolic
/// reference resolves at the same boundary against the live entities' symbolic
/// identifiers, so it sees earlier commands of the batch, and rejects with
/// [`ErrorReason::MissingSymbolicId`](crate::ErrorReason::MissingSymbolicId)
/// when no live entity carries the identifier.
/// [`BatchOutcome::symbols`](crate::BatchOutcome::symbols) reports the handle
/// each one resolved to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntityRef {
    /// World-local generational identity.
    Handle(EntityId),
    /// Entity created by an earlier [`Command::Create`] under this alias.
    Alias(u32),
    /// Live entity whose metadata carries this symbolic identifier.
    Symbol(std::sync::Arc<str>),
}

/// A typed exposed-field value, never a raw byte write.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
    /// Ingress-only World token, resolved at its ordered operation boundary.
    UnresolvedWorld(crate::WorldReferenceToken),
    /// Ingress-only output token, resolved before owning System callbacks.
    UnresolvedOutput(crate::OutputReferenceToken),
    /// Exact Host World lifetime, not a local entity or batch alias.
    World(Option<crate::WorldRef>),
    /// Exact selected output, including the producer component incarnation.
    Output(Option<crate::OutputRef>),
    /// Resolved dynamic property payload.
    Dynamic(crate::DynamicValue),
    /// Finite scalar value.
    F32(f32),
    /// Unsigned numeric field.
    U32(u32),
    /// Unsigned 64-bit field.
    U64(u64),
    /// Shared immutable UTF-8 field.
    String(std::sync::Arc<str>),
    /// Owned byte collection.
    Bytes(Vec<u8>),
    /// Boolean field.
    Bool(bool),
    /// Entity reference; batch aliases are resolved before retention.
    Entity(EntityRef),
    /// Whole schema rows table in the [`Rows`](crate::components::rows::Rows) encoding.
    Rows(Vec<u8>),
    /// Clear an optional schema row property.
    Unset,
}

/// Exact target-layout field address and owned value.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldWrite {
    /// Exact exposed offset in the compiled component.
    pub offset: u32,
    /// Value of the matching field type.
    pub value: FieldValue,
}

/// Authoring metadata, outside component storage.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EntityMetadata {
    /// Optional scene-unique symbolic identifier.
    pub symbolic_id: Option<String>,
    /// Class labels; preparation sorts and deduplicates them.
    pub classes: Vec<String>,
}

/// An ordered mutation. Operations apply in order and errors retain partial changes.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Ingress-only session receipt, resolved immediately before its ordered operation.
    DetachWorldAttachmentReceipt {
        /// Untrusted receipt identity in the submitting session's bounded registry.
        receipt: u64,
    },
    /// Create an entity and make its alias available to later operations.
    ///
    /// With `adopt`, a live entity that already carries the metadata's symbolic
    /// identifier is bound to the alias instead of failing with
    /// [`ErrorReason::DuplicateSymbolicId`](crate::ErrorReason::DuplicateSymbolicId),
    /// and receives the metadata as [`Command::SetMetadata`] would write it.
    /// Without a live match, or without a symbolic identifier, adoption creates
    /// the entity as usual.
    Create {
        /// Batch-local alias.
        alias: u32,
        /// Initial authoring metadata.
        metadata: EntityMetadata,
        /// Bind an existing entity with the same symbolic identifier.
        adopt: bool,
    },
    /// Delete a live entity and invalidate its bindings.
    Delete {
        /// Entity to delete.
        entity: EntityRef,
    },
    /// Resolve an ordered placement once, preserving local component values.
    PlaceEntity {
        /// Entity whose relationship changes.
        entity: EntityRef,
        /// Resolved once against the current siblings.
        placement: crate::EntityPlacementRef,
    },
    /// Explicitly remove the current subtree through ordinary lifecycle cleanup.
    DeleteSubtree {
        /// Effective subtree root, including this entity.
        root: EntityRef,
    },
    /// Replace all metadata.
    SetMetadata {
        /// Entity to update.
        entity: EntityRef,
        /// New metadata.
        metadata: EntityMetadata,
    },
    /// Create or replace a component, establishing a new incarnation.
    ///
    /// With `adopt`, an existing component keeps its incarnation and receives
    /// only the listed fields in place, validated together so an invalid write
    /// has no effect; an absent component is inserted as usual.
    InsertComponent {
        /// Target entity.
        entity: EntityRef,
        /// Compiled registry ID.
        component: u16,
        /// Initial writes over compiled defaults.
        fields: Vec<FieldWrite>,
        /// Write the listed fields of an existing component instead of replacing it.
        adopt: bool,
    },
    /// Native subsystem insertion with complete authored inputs. This operation
    /// does not require a default factory and has no production wire tag.
    /// Construct it with [`Command::insert_value`].
    InsertComponentValue {
        /// Target entity.
        entity: EntityRef,
        /// Registered typed authored inputs; effective resources are prepared
        /// locally. Boxed so the largest component never sets the inline size of
        /// every queued command (see [`MAX_COMMAND_INLINE_BYTES`]).
        value: Box<crate::ComponentValue>,
    },
    /// Update one field without replacing the component incarnation.
    SetField {
        /// Target entity.
        entity: EntityRef,
        /// Compiled registry ID.
        component: u16,
        /// Exact typed field write.
        field: FieldWrite,
    },
    /// Compare-and-set: write one field only while it still holds `expected`.
    ///
    /// The stored value is compared with `expected` of the field's own type, text
    /// by shared reference first and then by content. A differing value fails the
    /// operation with
    /// [`ErrorReason::ValueMismatch`](crate::ErrorReason::ValueMismatch), without
    /// effect, and stops the batch. Construct it with [`Command::set_field_if`].
    SetFieldIf {
        /// Target entity.
        entity: EntityRef,
        /// Compiled registry ID.
        component: u16,
        /// Exact typed field write applied when the comparison holds.
        field: FieldWrite,
        /// Value the field must hold, at the same offset as `field`. Boxed so the
        /// comparison value never sets the inline size of every queued command.
        expected: Box<FieldValue>,
    },
    /// Define, update or retype one named property.
    SetDynamicProperty {
        /// Target entity.
        entity: EntityRef,
        /// Compiled component identity supporting named properties.
        component: u16,
        /// Authored property name.
        name: String,
        /// Typed value; a changed type starts a new property lifetime.
        value: crate::DynamicValue,
    },
    /// Remove one named property, invalidating its bindings.
    RemoveDynamicProperty {
        /// Target entity.
        entity: EntityRef,
        /// Compiled component identity supporting named properties.
        component: u16,
        /// Authored property name.
        name: String,
    },
    /// Remove a component and invalidate its bindings.
    RemoveComponent {
        /// Target entity.
        entity: EntityRef,
        /// Compiled registry ID.
        component: u16,
    },
    /// Apply one semantic action to a GUI control at this operation's mutation
    /// boundary.
    ///
    /// The command is validated in order, each failure stopping the batch
    /// without effect: the entity resolves and holds the target control
    /// component at its incarnation
    /// ([`ErrorReason::StaleTarget`](crate::ErrorReason::StaleTarget)); the
    /// stored `GuiBehavior` eligibility fields are all set
    /// ([`ErrorReason::Unavailable`](crate::ErrorReason::Unavailable)); the
    /// action matches the control role
    /// ([`ErrorReason::UnsupportedAction`](crate::ErrorReason::UnsupportedAction));
    /// and the resulting value passes the field's constraints
    /// ([`ErrorReason::InvalidValue`](crate::ErrorReason::InvalidValue)). Value
    /// actions write the control's fields like [`Command::SetField`]; press,
    /// submit, focus and blur record a momentary effect that the GUI System
    /// publishes in its frame phase.
    GuiAction {
        /// Exact control lifetime the client observed.
        target: GuiActionTarget,
        /// Requested action.
        action: crate::systems::gui::local::GuiLocalAction,
    },
    /// Remove only the exact acknowledged attachment producer, preserving replacements.
    /// Both detached and superseded effects retain the original retirement observation.
    DetachWorldAttachmentIf {
        /// An owned applied-write receipt, resolved by the adapter's session registry.
        expected: crate::WorldAttachmentToken,
    },
}

/// The control a [`Command::GuiAction`] names: an entity and the exact
/// lifetime of its control component.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuiActionTarget {
    /// Control entity, by handle, batch alias or symbolic identifier.
    pub entity: EntityRef,
    /// Control component identity, which names the control role.
    pub component: u16,
    /// Component incarnation; a replaced component rejects the action.
    pub incarnation: u64,
}

/// Upper bound on the inline size of one [`Command`].
///
/// Queued batches keep one `Command` slot per operation, and
/// [`WorldLimits::max_batch_bytes`](crate::WorldLimits::max_batch_bytes)
/// charges `size_of::<Command>()` for every slot of operation capacity. Payloads
/// that grow with components (whole component values, field lists and names)
/// therefore live behind a heap pointer so that component growth never widens
/// every queued command. A compile-time assertion keeps every build, including
/// test builds with test-only registry components, within this bound.
pub const MAX_COMMAND_INLINE_BYTES: usize = 128;

const _: () = assert!(std::mem::size_of::<Command>() <= MAX_COMMAND_INLINE_BYTES);

impl Command {
    /// Visit every entity reference of this command, including placement
    /// references and entity-valued field writes, in operand order.
    ///
    /// The match is exhaustive: a new command variant must declare its references
    /// here, so symbolic references resolve wherever a command can carry one.
    pub(crate) fn visit_entity_refs_mut<E>(
        &mut self,
        visit: &mut impl FnMut(&mut EntityRef) -> Result<(), E>,
    ) -> Result<(), E> {
        fn field<E>(
            value: &mut FieldValue,
            visit: &mut impl FnMut(&mut EntityRef) -> Result<(), E>,
        ) -> Result<(), E> {
            if let FieldValue::Entity(reference) = value {
                visit(reference)?;
            }
            Ok(())
        }

        fn placement<E>(
            placement: &mut crate::EntityPlacementRef,
            visit: &mut impl FnMut(&mut EntityRef) -> Result<(), E>,
        ) -> Result<(), E> {
            for reference in [&mut placement.parent, &mut placement.before]
                .into_iter()
                .flatten()
            {
                visit(reference)?;
            }
            Ok(())
        }

        match self {
            Self::DetachWorldAttachmentReceipt {
                ..
            }
            | Self::Create {
                ..
            }
            | Self::DetachWorldAttachmentIf {
                ..
            } => Ok(()),
            Self::Delete {
                entity,
            }
            | Self::DeleteSubtree {
                root: entity,
            }
            | Self::SetMetadata {
                entity,
                ..
            }
            | Self::InsertComponentValue {
                entity,
                ..
            }
            | Self::SetDynamicProperty {
                entity,
                ..
            }
            | Self::RemoveDynamicProperty {
                entity,
                ..
            }
            | Self::RemoveComponent {
                entity,
                ..
            } => visit(entity),
            Self::PlaceEntity {
                entity,
                placement: target,
            } => {
                visit(entity)?;
                placement(target, visit)
            }
            Self::InsertComponent {
                entity,
                fields,
                ..
            } => {
                visit(entity)?;
                for write in fields {
                    field(&mut write.value, visit)?;
                }
                Ok(())
            }
            Self::SetField {
                entity,
                field: write,
                ..
            } => {
                visit(entity)?;
                field(&mut write.value, visit)
            }
            Self::SetFieldIf {
                entity,
                field: write,
                expected,
                ..
            } => {
                visit(entity)?;
                field(&mut write.value, visit)?;
                field(expected, visit)
            }
            Self::GuiAction {
                target,
                ..
            } => visit(&mut target.entity),
        }
    }
}

impl Command {
    /// Insert a complete typed component value; see [`Command::InsertComponentValue`].
    pub fn insert_value(entity: EntityRef, value: crate::ComponentValue) -> Self {
        Self::InsertComponentValue {
            entity,
            value: Box::new(value),
        }
    }

    /// Write `field` only while it holds `expected`; see [`Command::SetFieldIf`].
    pub fn set_field_if(
        entity: EntityRef,
        component: u16,
        field: FieldWrite,
        expected: FieldValue,
    ) -> Self {
        Self::SetFieldIf {
            entity,
            component,
            field,
            expected: Box::new(expected),
        }
    }
}
