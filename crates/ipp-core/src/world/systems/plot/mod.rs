//! Plot consumes completed entity-local bindings and publishes compact retained geometry.
//! Authored components remain the sole values; System state contains derived geometry only.

mod component;
mod frame_mapping;
mod interpolation_reference;
mod picking;
mod prepared_input;
mod primitive_preparation;
mod publication;
mod system;
mod system_state;
mod update;

pub mod plots_2d;
pub mod plots_3d;

pub use component::*;
pub use frame_mapping::{PlotFrameMapping2d, PlotFrameMapping3d};
pub use picking::{PlotCanvasHit, PlotRayHit};
pub use prepared_input::{PlotPreparedColumn, PlotPreparedInput};
pub use publication::*;
pub use system::{PlotSystem, PlotSystemFactory};
