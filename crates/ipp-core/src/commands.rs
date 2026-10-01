//! Owned native operations shared with transport codecs.

use crate::EntityId;

/// A permanent handle, an entity named by an earlier command of this logical
/// batch, or a live entity named by its symbolic identifier.
///
/// Aliases resolve in command order at each command's own mutation boundary and
/// behave exactly like the handle they name; a name that no earlier successful
/// command of the batch defined rejects with [`ErrorReason::UnknownAlias`]. A
/// symbolic reference resolves at the same boundary against the live entities'
/// symbolic identifiers, so it sees earlier commands of the batch, and rejects
/// with [`ErrorReason::MissingSymbolicId`] when no live entity carries the
/// identifier. [`BatchOutcome::symbols`] reports the handle each one resolved to.
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
    /// [`ErrorReason::DuplicateSymbolicId`], and receives the metadata as
    /// [`Command::SetMetadata`] would write it. Without a live match, or without a
    /// symbolic identifier, adoption creates the entity as usual.
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
    /// operation with [`ErrorReason::ValueMismatch`], without effect, and stops
    /// the batch. Construct it with [`Command::set_field_if`].
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
    /// component at its incarnation ([`ErrorReason::StaleTarget`]); the stored
    /// `GuiBehavior` eligibility fields are all set ([`ErrorReason::Unavailable`]);
    /// the action matches the control role ([`ErrorReason::UnsupportedAction`]);
    /// and the resulting value passes the field's constraints
    /// ([`ErrorReason::InvalidValue`]). Value actions write the control's fields
    /// like [`Command::SetField`]; press, submit, focus and blur record a
    /// momentary effect that the GUI System publishes in its frame phase.
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

/// One ordered command buffer, submitted alone or within a Host-controlled stream.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    /// Caller-supplied correlation identity.
    pub id: u64,
    /// Operations in submitted order.
    pub operations: Vec<Command>,
}

/// Stable semantic rejection reasons; codecs choose their own numeric tags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorReason {
    /// Commit cleanup did not converge; the World is faulted after safe release.
    NonConvergentCommit,
    /// No explicit camera selection exists in this world.
    NoActiveCamera,
    /// The operation would remove the active camera or a required component.
    ActiveCamera,
    /// The query position or dimensions are invalid or differ from the surface.
    InvalidViewport,
    /// A geometry consumer needs a definition or pose that is not available.
    GeometryUnavailable,
    /// Geometry, its transform or its mapping is unusable.
    InvalidGeometry,
    /// The handle fails bounds, generation, or liveness checks.
    InvalidEntity,
    /// No earlier successful command of this logical batch defined the alias.
    UnknownAlias,
    /// Alias was already used in this batch.
    DuplicateAlias,
    /// Another live entity owns the symbolic ID.
    DuplicateSymbolicId,
    /// Component is absent from this compiled registry.
    UnknownComponent,
    /// Required component is absent.
    MissingComponent,
    /// The component supports binding and writes but has no creation factory.
    MissingCreationContract,
    /// Offset or type is not an exact exposed field match.
    InvalidField,
    /// Value, timestep, or computed scalar is invalid.
    InvalidValue,
    /// Malformed mesh payload or invalid asset identity.
    InvalidAsset,
    /// Effective mesh reference is not ready.
    MissingAsset,
    /// Immutable key was already published.
    DuplicateAsset,
    /// An explicit memory, operation, identity, or clock budget is exhausted.
    Capacity,
    /// Dependency cannot be satisfied by fixed ascending-slot evaluation.
    UnsupportedDependency,
    /// No live entity carries the referenced symbolic identifier.
    MissingSymbolicId,
    /// A compare-and-set found a value other than the expected one; nothing changed.
    ValueMismatch,
    /// A GUI action's entity no longer holds the control component at the
    /// observed incarnation.
    StaleTarget,
    /// A GUI action's control is disabled, hidden or unavailable.
    Unavailable,
    /// A GUI action does not match the control's role.
    UnsupportedAction,
}

impl std::fmt::Display for ErrorReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ErrorReason {}

/// Boundary that rejected an ordered batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchErrorScope {
    /// A specific operation retained its partial effects and stopped execution.
    Operation,
    /// Applied operations completed but commit validation or cleanup failed.
    Commit,
}

/// Failure after applying zero or more operations; no rollback is performed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchError {
    /// Operation or commit boundary that failed.
    pub scope: BatchErrorScope,
    /// Zero-based originating operation when known; absent for unattributed commit errors.
    pub operation: Option<usize>,
    /// Semantic rejection reason.
    pub reason: ErrorReason,
    /// Entity aliases created before execution stopped, including a partial operation.
    pub aliases: Vec<(u32, EntityId)>,
}

/// Applied-buffer result. Complete batches publish after evaluation; streaming
/// Hosts may acknowledge buffers while withholding the next evaluated frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchOutcome {
    /// Submitted batch identity.
    pub batch_id: u64,
    /// Completed World tick at publication; a streamed buffer does not advance it.
    pub tick: u64,
    /// Creation aliases in creation order, or the failed operation.
    pub result: Result<Vec<(u32, EntityId)>, BatchError>,
    /// Handles that symbolic references resolved to, one entry per distinct
    /// symbol and handle in first-resolution order, including operations applied
    /// before a failure.
    pub symbols: Vec<(std::sync::Arc<str>, EntityId)>,
    /// Correlated applied effects, including those preceding an operation or commit failure.
    pub effects: Vec<AppliedOperationEffect>,
}

/// One operation's concrete applied effect, issued by World orchestration or by
/// the System that owns it; the batch outcome reports it at its operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperationEffect {
    /// Exact attachment producer identity and conditional-cleanup outcome.
    WorldAttachment(crate::WorldAttachmentEffect),
    /// An adopting operation found its target already present: an adopting
    /// [`Command::Create`] bound an existing entity, or an adopting
    /// [`Command::InsertComponent`] wrote an existing component in place. An
    /// adopting operation that created or inserted reports no effect.
    Adopted,
}

/// Delivery capacity required before one operation can mutate authoring state.
#[derive(Clone, Debug, Default)]
pub struct OperationEffectDemand {
    /// Maximum newly issued attachment tokens, separate from already retained identities.
    pub fresh_attachment_tokens: usize,
    /// Exact existing tokens that can be emitted, for registry-level deduplication.
    pub existing_attachment_tokens: Vec<crate::WorldAttachmentToken>,
    /// Maximum attachment effect records, including repeated observations of an existing token.
    pub max_records: usize,
    /// The operation adopts: it may report one [`OperationEffect::Adopted`] beyond
    /// `max_records`. Adoption reports carry no receipt, so a sink can admit them
    /// with their batch instead of per operation.
    pub adopting: bool,
}

/// Operation-scoped reliable effect admission; native safety cleanup does not use this sink.
pub trait OperationEffectSink {
    /// Reserve registry and delivery capacity before any System mutation callback.
    fn reserve(&mut self, demand: &OperationEffectDemand) -> Result<(), ErrorReason>;

    /// Retain one committed effect infallibly within the successful reservation.
    fn emit(&mut self, effect: OperationEffect);

    /// Release unused reservation on every operation exit, including failed reservation.
    fn settle(&mut self);

    /// Resolve an untrusted receipt in the current submitting session at execution time.
    fn attachment_receipt(
        &self,
        _receipt: u64,
    ) -> Result<crate::WorldAttachmentToken, ErrorReason> {
        Err(ErrorReason::InvalidEntity)
    }
}

/// An effect at its original operation position within the acknowledged command buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedOperationEffect {
    /// Zero-based operation index, including retained effects of a failed operation.
    pub operation: usize,
    /// Owned concrete effect issued by its subsystem.
    pub effect: OperationEffect,
}
