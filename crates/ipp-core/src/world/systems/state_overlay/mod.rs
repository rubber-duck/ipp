//! State overlay lifecycle and storage, compiled with the state-overlays feature.
//!
//! The world mutates this state alongside ordinary authored state; failures retain
//! partial declarations, ownership and resolved inputs. Shared world code
//! accesses component inputs through the same interface as the base-only build.

mod batch;

mod component_inputs;

mod component_requirements;

mod registry;

mod input_staging;
mod invalidation;
mod lifecycle;

mod asset_dependencies;

pub use registry::{
    ComponentOverlayMode, EntityOverlayMode, MAX_STATE_OVERLAY_DIAGNOSTICS, StateOverlayAlias,
    StateOverlayHandleKind, StateOverlayLifecycleDiagnostic, StateOverlayLifecycleReason,
    StateOverlayRef,
};

pub(crate) use batch::StateOverlayBatch;

pub(in crate::world) use component_inputs::ComponentStateOverlayInputs;

use registry::{StateOverlayEntry, StateOverlayFields, StateOverlayRegistry};

#[cfg(test)]
mod storage_tests;

#[cfg(test)]
mod preparation_tests;

mod system;
pub use system::{StateOverlaySystem, StateOverlaySystemFactory};

pub(in crate::world) struct StateOverlayMutationAccess<'a> {
    staged: &'a mut crate::world::WorldMutationState,
    state_overlays: &'a mut StateOverlaySystemState,
}

impl std::ops::Deref for StateOverlayMutationAccess<'_> {
    type Target = crate::world::WorldMutationState;

    fn deref(&self) -> &Self::Target {
        self.staged
    }
}

impl std::ops::DerefMut for StateOverlayMutationAccess<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.staged
    }
}

mod system_state;
pub use system_state::StateOverlaySystemState;

mod world_api;
