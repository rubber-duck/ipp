//! Root presentation selection and its explicit rebind lifetime.

use super::{OutputRef, WorldViewport};

/// Host-fenced identity of one successful explicit root binding. Never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RootBindingGeneration {
    pub(super) host: u64,
    pub(super) serial: u64,
}

impl RootBindingGeneration {
    /// Stable Host identity and bind serial for transport observation and exact comparison.
    /// Reading this pair does not authorize reconstructing or selecting a root binding.
    pub fn identity(self) -> (u64, u64) {
        (self.host, self.serial)
    }
}

/// The authoritative root selection. Publication refresh does not replace this record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RootOutputBinding {
    /// Exact explicitly selected producer lifetime.
    pub output: OutputRef,
    /// Current authored presentation dimensions and density.
    pub viewport: WorldViewport,
    /// Identity replaced by every successful explicit bind, including equal values.
    pub generation: RootBindingGeneration,
}
