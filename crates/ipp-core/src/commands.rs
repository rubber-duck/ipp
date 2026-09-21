//! Owned native operations shared with transport codecs.

use crate::EntityId;
use crate::{ComponentOverlayMode, EntityOverlayMode, StateOverlayAlias, StateOverlayRef};

/// A permanent handle or an earlier creation in this batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityRef {
    /// World-local generational identity.
    Handle(EntityId),
    /// Batch-local creation alias.
    Alias(u32),
}

/// A typed exposed-field value, never a raw byte write.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
    /// Resolved dynamic property payload.
    Dynamic(crate::DynamicValue),
    /// Finite scalar value.
    F32(f32),
    /// Unsigned numeric field.
    U32(u32),
    /// Unsigned 64-bit field.
    U64(u64),
    /// Owned UTF-8 field.
    String(String),
    /// Owned byte collection.
    Bytes(Vec<u8>),
    /// Boolean field.
    Bool(bool),
    /// Entity reference; aliases are resolved before retention.
    Entity(EntityRef),
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
    /// Create an entity and make its alias available to later operations.
    Create {
        /// Batch-local alias.
        alias: u32,
        /// Initial authoring metadata.
        metadata: EntityMetadata,
    },
    /// Delete a live entity and invalidate its bindings.
    Delete {
        /// Entity to delete.
        entity: EntityRef,
    },
    /// Replace all metadata.
    SetMetadata {
        /// Entity to update.
        entity: EntityRef,
        /// New metadata.
        metadata: EntityMetadata,
    },
    /// Create or replace a producer component, establishing a new incarnation.
    InsertComponent {
        /// Target entity.
        entity: EntityRef,
        /// Compiled registry ID.
        component: u16,
        /// Initial writes over compiled defaults.
        fields: Vec<FieldWrite>,
    },
    /// Native subsystem insertion with complete authored inputs. This operation
    /// does not require a default factory and has no production wire tag.
    InsertComponentValue {
        /// Target entity.
        entity: EntityRef,
        /// Registered typed authored inputs; effective resources are prepared locally.
        value: crate::ComponentValue,
    },
    /// Update a producer base field without replacing its incarnation.
    SetField {
        /// Target entity.
        entity: EntityRef,
        /// Compiled registry ID.
        component: u16,
        /// Exact typed field write.
        field: FieldWrite,
    },
    /// Define, update or retype one named producer property.
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
    /// Remove one named producer property, invalidating its bindings.
    RemoveDynamicProperty {
        /// Target entity.
        entity: EntityRef,
        /// Compiled component identity supporting named properties.
        component: u16,
        /// Authored property name.
        name: String,
    },
    /// Remove a producer component and invalidate its bindings.
    RemoveComponent {
        /// Target entity.
        entity: EntityRef,
        /// Compiled registry ID.
        component: u16,
    },
    /// Create a producer scope for persistent resources.
    CreateStateOverlayOwner {
        /// Batch-local resource alias.
        alias: u32,
    },
    /// Release all resources belonging to this producer scope.
    ReleaseStateOverlayOwner {
        /// Owner scope.
        owner: StateOverlayRef,
    },
    /// Create or bind a symbolic entity with an explicit lifetime policy.
    AttachEntityOverlayBinding {
        /// Live owner scope.
        owner: StateOverlayRef,
        /// Batch-local resource alias.
        alias: u32,
        /// World-unique symbolic identifier.
        symbolic_id: String,
        /// Entity lifetime policy.
        mode: EntityOverlayMode,
    },
    /// Release an entity association and its declarations.
    ReleaseEntityOverlayBinding {
        /// Original owner scope.
        owner: StateOverlayRef,
        /// Entity binding to release.
        binding: StateOverlayRef,
    },
    /// Retain a sparse component declaration under an entity association.
    AttachComponentStateOverlay {
        /// Live owner scope.
        owner: StateOverlayRef,
        /// Live entity association belonging to that owner.
        binding: StateOverlayRef,
        /// Batch-local resource alias.
        alias: u32,
        /// Compiled component ID.
        component: u16,
        /// Component lifetime policy.
        mode: ComponentOverlayMode,
        /// Initial sparse field replacements.
        fields: Vec<FieldWrite>,
    },
    /// Change listed replacements and withdraw explicitly cleared fields.
    UpdateComponentStateOverlay {
        /// Live owner scope.
        owner: StateOverlayRef,
        /// Live declaration belonging to that owner.
        overlay: StateOverlayRef,
        /// New values for listed fields.
        fields: Vec<FieldWrite>,
        /// Exact exposed field offsets to withdraw before applying writes.
        clear: Vec<u32>,
    },
    /// Update sparse overrides by dynamic property name.
    UpdateDynamicComponentStateOverlay {
        /// Live owner scope.
        owner: StateOverlayRef,
        /// Existing declaration belonging to that owner.
        overlay: StateOverlayRef,
        /// Named values; types must match existing properties.
        properties: Vec<(String, crate::DynamicValue)>,
        /// Names whose contributions are withdrawn.
        clear: Vec<String>,
    },
    /// Withdraw a declaration and its owned lifetime demand.
    ReleaseComponentStateOverlay {
        /// Original owner scope.
        owner: StateOverlayRef,
        /// Declaration to release.
        overlay: StateOverlayRef,
    },
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
    /// Alias has not been created earlier in this batch.
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
    /// Resource handle, kind, or active association is invalid.
    InvalidStateOverlay,
    /// A live resource belongs to another owner scope.
    StateOverlayOwnershipMismatch,
    /// A bound symbolic entity does not exist.
    MissingSymbolicId,
    /// Owned creation requires both base and effective component to be absent.
    ComponentExists,
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
    /// Scoped resource aliases in attachment order, including work applied before failure.
    pub state_overlays: Vec<StateOverlayAlias>,
}
