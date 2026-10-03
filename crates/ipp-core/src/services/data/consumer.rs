use crate::{EntityId, WorldRef, components::DynamicValue};

use super::{DataError, DataSourceHandle, DataSourceKind};

/// Binding identity includes the World and component lifetimes, never just an entity slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataConsumerIdentity {
    /// Exact World lifetime owning this demand.
    pub world: WorldRef,
    /// World-local entity carrying the binding.
    pub entity: EntityId,
    /// Binding component lifetime, supplied by the owning World.
    pub binding_incarnation: u64,
}

/// Opaque authority for one consumer registration, never retargeted after release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataConsumerHandle {
    pub(super) service: u64,
    pub(super) serial: u64,
}

/// Numeric time columns use caller-selected units on existing core numeric kinds.
#[derive(Clone, Debug, PartialEq)]
pub enum DataWindowAnchor {
    /// Forward-moving anchor supplied in the raw column's exact core numeric kind.
    Supplied(DynamicValue),
    /// Maximum committed value of the raw column, including already expired rows.
    Latest,
    /// Host elapsed seconds multiplied by this finite positive unit conversion.
    HostTime {
        /// Finite positive conversion from elapsed seconds to raw column units.
        units_per_second: f64,
    },
}

/// Constraints intersect within a consumer; range endpoints are inclusive.
/// Window updates match repeated raw-column anchors in occurrence order.
#[derive(Clone, Debug, PartialEq)]
pub enum DataWindow {
    /// Select the last N committed arrivals by insertion order; zero selects none.
    Count(usize),
    /// Inclusive raw numeric interval from anchor minus width through anchor.
    Range {
        /// Named raw scalar numeric column.
        column: String,
        /// Finite nonnegative width in the raw column's units.
        width: f64,
        /// Forward-moving supplied, data-driven or Host-time endpoint.
        anchor: DataWindowAnchor,
    },
}

/// Source selection and raw windows authored by one binding.
#[derive(Clone, Debug, PartialEq)]
pub struct DataConsumerRequest {
    /// Stable name used for schema or Host-wide source resolution.
    pub name: String,
    /// Exact existing core value type or source kind.
    pub kind: DataSourceKind,
    /// Buffer consumers must leave this empty; streams without windows use the byte cap.
    pub windows: Vec<DataWindow>,
}

/// Observable name, kind and raw-column readiness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataAvailability {
    /// The name, source kind and raw-column constraints are compatible.
    Ready,
    /// Resolution failure reported to this consumer.
    Unavailable(DataError),
}

/// Current resolution of one name-bound consumer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataConsumerState {
    /// Current fenced source, including incompatible sources when resolution succeeded.
    pub source: Option<DataSourceHandle>,
    /// Whether this consumer can read its requested source and columns.
    pub availability: DataAvailability,
}

/// Coalesced pending observation consumed by the binding at its mutation boundary.
/// This is not a source value revision or the binding's presentation dirty flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataConsumerNotification {
    /// Current readiness and source incarnation at the notification cut.
    pub state: DataConsumerState,
    /// Source data, selection or availability changed since the previous consumption.
    pub changed: bool,
    /// Source identity or readiness changed since the previous consumption.
    pub availability_changed: bool,
}

pub(super) struct DataConsumer {
    pub handle: DataConsumerHandle,
    pub identity: DataConsumerIdentity,
    pub request: DataConsumerRequest,
    /// Current readiness and source incarnation at the notification cut.
    pub state: DataConsumerState,
    // A widened integer bookmark represents the sentinel after u64::MAX without reuse.
    pub default_start: u128,
    pub pending_changed: bool,
    pub pending_availability: bool,
}

impl DataConsumerRequest {
    /// Validate portable authoring syntax without resolving a source or Host clock.
    /// Consumers can preflight stored configuration without registering demand.
    pub fn validate(&self) -> Result<(), DataError> {
        super::retention::validate_request(self)
    }
}
