//! Terminal object aiming. Joint-local constraints are not an authored target here.

mod component;
pub use component::{LookAt, LookAtRuntimeState};

mod math;

mod system;
pub use system::{LookAtSystem, LookAtSystemFactory};

mod system_state;
