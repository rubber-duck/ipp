use super::{ComponentLifecycleKind, EntityLifecycleKind, LifecycleObservation};
use crate::components::schema::{FieldKind, FieldValue};
use crate::{ComponentValue, EntityId, WorldRef};
use std::sync::Arc;

/// One exact generational entity, entity/type pair or set of a component's
/// schema fields; lifecycle targets retain no component value.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LifecycleWatchTarget {
    /// Entity transitions only.
    Entity(EntityId),
    /// Effective component transitions only; the entity generation never follows reuse.
    Component(EntityId, u16),
    /// Current values of these fields of one component, reported with
    /// [`LifecycleWatchRecordBody::Value`] whenever they differ at frame end from
    /// the values last reported to the member. Offsets ascend strictly and name
    /// exposed schema fields of the component, never rows or named properties;
    /// the entity generation never follows reuse.
    Value(EntityId, u16, Arc<[u32]>),
}

impl LifecycleWatchTarget {
    pub(super) fn valid(&self) -> bool {
        match self {
            Self::Entity(entity) => entity.to_bits() != 0,
            Self::Component(entity, component) => entity.to_bits() != 0 && *component != 0,
            Self::Value(entity, component, fields) => {
                entity.to_bits() != 0
                    && *component != 0
                    && !fields.is_empty()
                    && fields.windows(2).all(|pair| pair[0] < pair[1])
                    && fields
                        .iter()
                        .all(|&offset| Self::value_field(*component, offset))
            }
        }
    }

    /// Whether a value target may observe this field: an exposed schema field,
    /// not a rows table, row property or named property.
    fn value_field(component: u16, offset: u32) -> bool {
        !crate::components::dynamic_properties::is_dynamic_field(offset)
            && crate::components::rows::row_region(offset).is_none()
            && ComponentValue::has_field(component, offset)
            && ComponentValue::validate_field(component, offset, FieldKind::Rows).is_err()
    }

    pub(super) fn observation(observation: &LifecycleObservation) -> Option<Self> {
        match observation {
            LifecycleObservation::Entity {
                entity,
                ..
            } => Some(Self::Entity(*entity)),
            LifecycleObservation::Component {
                entity,
                component,
                ..
            } => Some(Self::Component(*entity, *component)),
            LifecycleObservation::Asset {
                ..
            } => None,
        }
    }
}

/// Validated entity/component transition mask, applied before output allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LifecycleWatchKinds(u8);

impl LifecycleWatchKinds {
    /// Fresh entity identities.
    pub const ENTITY_CREATED: Self = Self(1);
    /// Changed authored entity metadata.
    pub const ENTITY_METADATA_CHANGED: Self = Self(2);
    /// Retired entity identities.
    pub const ENTITY_DELETED: Self = Self(4);
    /// Previously absent effective components.
    pub const COMPONENT_INSERTED: Self = Self(8);
    /// Value edits within an incarnation.
    pub const COMPONENT_UPDATED: Self = Self(16);
    /// Effective incarnation replacements.
    pub const COMPONENT_REPLACED: Self = Self(32);
    /// Removed effective components.
    pub const COMPONENT_REMOVED: Self = Self(64);
    /// Component lifetime loss, excluding insertion and value edits.
    pub const COMPONENT_RETIRED: Self = Self(32 | 64);
    /// Changed field values of a value target, the only kind a value target selects.
    pub const VALUE_CHANGED: Self = Self(128);

    /// Reject empty wire-independent bit sets.
    pub fn from_bits(bits: u8) -> Option<Self> {
        (bits != 0).then_some(Self(bits))
    }

    /// Stable semantic bits; codecs own their wire representation.
    pub fn bits(self) -> u8 {
        self.0
    }

    pub(super) fn valid_for(self, target: &LifecycleWatchTarget) -> bool {
        let supported = match target {
            LifecycleWatchTarget::Entity(_) => 7,
            LifecycleWatchTarget::Component(_, _) => 120,
            LifecycleWatchTarget::Value(_, _, _) => 128,
        };
        self.0 != 0 && self.0 & !supported == 0
    }

    pub(super) fn matches(self, observation: &LifecycleObservation) -> bool {
        let bit = match observation {
            LifecycleObservation::Entity {
                kind,
                ..
            } => match kind {
                EntityLifecycleKind::Created => 1,
                EntityLifecycleKind::MetadataChanged => 2,
                EntityLifecycleKind::Deleted => 4,
            },
            LifecycleObservation::Component {
                kind,
                ..
            } => match kind {
                ComponentLifecycleKind::Inserted => 8,
                ComponentLifecycleKind::Updated => 16,
                ComponentLifecycleKind::Replaced => 32,
                ComponentLifecycleKind::Removed => 64,
            },
            LifecycleObservation::Asset {
                ..
            } => 0,
        };
        self.0 & bit != 0
    }
}

/// An output lifetime and monotonically minted member generation; never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LifecycleWatchId {
    /// Unique output endpoint lifetime.
    pub output: u64,
    /// Nonzero generation within that endpoint.
    pub generation: u64,
}

/// Identity read at the command's apply cut, independent of later publication.
///
/// A value target reports the lifetime of its component, as a component target does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleTargetLifetime {
    /// Entity membership includes whether that exact generation exists.
    Entity {
        /// Whether the exact entity generation is currently occupied.
        live: bool,
    },
    /// Missing entity and missing component remain distinguishable.
    Component {
        /// Whether the containing entity generation is currently occupied.
        entity_live: bool,
        /// Effective incarnation, absent when either entity or component is missing.
        incarnation: Option<u64>,
    },
    /// This member generation has been withdrawn.
    Removed,
}

/// One member's owned result at an acknowledged membership cut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleMembershipBaseline {
    /// Exact member generation, also present on subsequent events.
    pub member: LifecycleWatchId,
    /// Exact requested target.
    pub target: LifecycleWatchTarget,
    /// Frozen identity observation; never fetched from current storage on drain.
    pub lifetime: LifecycleTargetLifetime,
}

/// A page changes membership atomically after validation at ordered ingress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleMembershipAction {
    /// Activate inert members and observe their current lifetimes.
    Add,
    /// Withdraw exactly these generations, preserving other users of each target.
    Remove,
}

/// Membership rejection never partially activates a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleMembershipRejection {
    /// The receiver World lifetime differs from the fixed output World.
    StaleWorld,
    /// The receiver session differs from the fixed output session.
    StaleSession,
    /// A retired member cannot be added again; mint a fresh generation.
    StaleMember,
    /// A member in this add page is already active.
    AlreadyActive,
    /// Tracking ended with session release or output closure.
    TrackingEnded,
    /// The existing World session admission bound was reached.
    Capacity,
}

/// The sole correlated reply, retained in order with the member's observations.
#[derive(Debug, PartialEq, Eq)]
pub enum LifecycleMembershipResult {
    /// One immutable result per requested member, in request order.
    Applied(Vec<LifecycleMembershipBaseline>),
    /// No membership changes occurred.
    Rejected(LifecycleMembershipRejection),
    /// The prepared command was discarded before application.
    Cancelled,
}

/// Adapter-declared peak encoding allocations; Core owns no wire limits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LifecycleWatchEncoding {
    /// Fixed encoded capacity for an ACK page, including rejection/cancellation.
    pub acknowledgement_bytes: usize,
    /// Additional encoded capacity for each baseline in an ACK page.
    pub baseline_bytes: usize,
    /// Encoded capacity of one entity/component event.
    pub event_bytes: usize,
    /// Additional baseline capacity for each field offset of a value target.
    pub baseline_field_bytes: usize,
    /// Encoded capacity of a value record without its field values.
    pub value_bytes: usize,
    /// Encoded capacity of one reported field value, excluding the length of
    /// text and byte payloads, which add their own length.
    pub value_field_bytes: usize,
}

impl LifecycleWatchEncoding {
    /// Encoded capacity of one baseline for `target`, or `None` on overflow.
    pub fn baseline_capacity(&self, target: &LifecycleWatchTarget) -> Option<usize> {
        match target {
            LifecycleWatchTarget::Entity(_) | LifecycleWatchTarget::Component(_, _) => {
                Some(self.baseline_bytes)
            }
            LifecycleWatchTarget::Value(_, _, fields) => self
                .baseline_field_bytes
                .checked_mul(fields.len())?
                .checked_add(self.baseline_bytes),
        }
    }

    /// Encoded capacity of one value record carrying `values`, or `None` on
    /// overflow or for a value kind that value targets cannot select.
    pub fn value_capacity(&self, values: Option<&[(u32, FieldValue)]>) -> Option<usize> {
        values
            .unwrap_or_default()
            .iter()
            .try_fold(self.value_bytes, |bytes, (_, value)| {
                let payload = match value {
                    FieldValue::String(text) => text.len(),
                    FieldValue::Bytes(bytes) => bytes.len(),
                    FieldValue::Rows(_) | FieldValue::Dynamic(_) => return None,
                    FieldValue::World(_)
                    | FieldValue::Output(_)
                    | FieldValue::F32(_)
                    | FieldValue::Entity(_)
                    | FieldValue::U32(_)
                    | FieldValue::U64(_)
                    | FieldValue::Bool(_)
                    | FieldValue::Unset => 0,
                };
                bytes
                    .checked_add(self.value_field_bytes)?
                    .checked_add(payload)
            })
    }
}

/// Owned output payload; each record travels with its non-Clone accounting lease.
#[derive(Debug, PartialEq)]
pub struct LifecycleWatchRecord {
    /// Fixed runtime World lifetime.
    pub world: WorldRef,
    /// Fixed authoring-session lifetime.
    pub session: u64,
    /// Correlated control or asynchronous observation.
    pub body: LifecycleWatchRecordBody,
}

/// ACKs remain deliverable when membership removal or retirement discards observations.
#[derive(Debug, PartialEq)]
pub enum LifecycleWatchRecordBody {
    /// Applied identity cut; a prepared cancellation has no applied sequence/tick.
    Acknowledgement {
        /// Adapter request correlation, distinct from the member generation.
        request: u64,
        /// Requested page operation.
        action: LifecycleMembershipAction,
        /// World sequence/tick at apply, or absent for cancellation before ingress.
        cut: Option<(u64, u64)>,
        /// Frozen result data.
        result: LifecycleMembershipResult,
    },
    /// Every matched transition, with no coalescing or inferred history.
    Event {
        /// Generation active when this transition was selected.
        member: LifecycleWatchId,
        /// World sequence; filters may create gaps.
        sequence: u64,
        /// Mutation boundary of the transition.
        tick: u64,
        /// Owned entity/component transition.
        observation: LifecycleObservation,
    },
    /// Current values of a value member's fields at the end of an evaluated
    /// frame. The first report after a member's ACK carries its current values;
    /// later ones follow a frame whose final values differ from the last report.
    /// Values are state: a newer record supersedes an undelivered one.
    Value {
        /// Value member generation.
        member: LifecycleWatchId,
        /// Evaluated tick whose final stored values these are.
        tick: u64,
        /// `(offset, value)` in the target's field order, or `None` while the
        /// entity or component is absent.
        values: Option<Vec<(u32, FieldValue)>>,
    },
}
