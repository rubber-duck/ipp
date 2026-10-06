//! Ordered command buffers and their applied outcomes.

use super::{AppliedOperationEffect, Command, ErrorReason};
use crate::EntityId;

/// One ordered command buffer, submitted alone or within a Host-controlled stream.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    /// Caller-supplied correlation identity.
    pub id: u64,
    /// Operations in submitted order.
    pub operations: Vec<Command>,
}

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
