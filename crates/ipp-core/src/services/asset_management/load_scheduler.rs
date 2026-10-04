//! Executor-neutral scheduling boundary for owned resource operations.

use std::{future::Future, pin::Pin};

/// Owned load task. Dropping this lease cancels its future without releasing
/// buffers still owned by an outstanding platform operation.
pub trait AssetLoadTask {}

/// Host-local scheduling supplied before resource acquisition starts.
/// The Host executor supplies real readiness wakers and owns cancellation/draining.
pub trait AssetLoadScheduler {
    /// Schedule owned acquisition/decoding work; the task never borrows a World.
    fn spawn(&self, future: Pin<Box<dyn Future<Output = ()> + 'static>>) -> Box<dyn AssetLoadTask>;
}
