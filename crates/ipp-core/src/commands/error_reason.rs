//! Stable semantic rejection reasons.

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
