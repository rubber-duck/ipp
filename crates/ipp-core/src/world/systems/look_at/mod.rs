//! Terminal object aiming. Joint-local constraints are not an authored target here.

mod component;
pub use component::{LookAt, LookAtRuntimeState};

mod entity_references;

mod math;

mod system;
pub use system::{LookAtSystem, LookAtSystemFactory};

mod system_state;
#[cfg(test)]
pub(crate) use system_state::LOOK_AT_CHECKS;
