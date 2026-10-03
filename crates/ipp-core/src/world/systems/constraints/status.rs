//! Read-only expression driver observations; querying performs no work.

use crate::{EntityId, ErrorReason};

/// Availability of the immutable expression payload, separate from sample validity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpressionDriverAvailability {
    /// Asset demand has not produced a decoded payload yet.
    Pending,
    /// A decoded immutable plan is available.
    Ready,
    /// Payload unloaded, failed or identity removed.
    Unavailable,
}

/// Why the driver retained its destination's stored value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpressionDriverReason {
    /// Asset pending, unloaded or absent.
    AssetUnavailable,
    /// Asset decoding/loading failed.
    AssetFailed,
    /// Mapping names do not match declared expression inputs.
    InputMapping,
    /// The source property's exact kind does not match the declared slot.
    InputType {
        /// Declared expression input slot.
        slot: u32,
    },
    /// A reached input property is absent or its pinned incarnation departed.
    MissingInput {
        /// Declared expression input slot.
        slot: u32,
    },
    /// A reached input contains an invalid typed value.
    InvalidInput {
        /// Declared expression input slot.
        slot: u32,
    },
    /// Arithmetic failure such as division by zero or overflow.
    Calculation,
    /// Destination missing, not eligible, wrong output type or incarnation departed.
    TargetUnavailable,
    /// Typed destination validation refused this result before notification/write.
    TargetRejected(ErrorReason),
    /// This driver belongs to a mixed property-dependency cycle.
    Cycle,
}

/// Result of the last preparation/evaluation boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpressionDriverState {
    /// Ready to evaluate but no result has been published yet.
    Prepared,
    /// A successful absolute write was published.
    Written,
    /// No write; stored target and remembered animation contribution were retained.
    Retained(ExpressionDriverReason),
}

/// Transient observation, reconstructed from authored declarations after restore.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpressionDriverStatus {
    /// Asset payload availability.
    pub availability: ExpressionDriverAvailability,
    /// Last committed driver result.
    pub state: ExpressionDriverState,
    /// Last evaluation successfully recovered from a retained result.
    /// Remains that observation until the next evaluation; query never consumes it.
    pub recovered: bool,
}

impl Default for ExpressionDriverStatus {
    fn default() -> Self {
        Self {
            availability: ExpressionDriverAvailability::Pending,
            state: ExpressionDriverState::Retained(ExpressionDriverReason::AssetUnavailable),
            recovered: false,
        }
    }
}

impl crate::WorldContext<'_> {
    /// Observe one present expression driver, without preparing, advancing or writing.
    pub fn expression_driver_status(&self, entity: EntityId) -> Option<ExpressionDriverStatus> {
        self.system::<super::ConstraintSystem>(super::ConstraintSystem::ID)?
            .state
            .expressions
            .get(&entity)
            .map(|binding| binding.status.clone())
    }
}
