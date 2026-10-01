//! World canvas selection shared by unit tests that present or evaluate a canvas.

use crate::{CanvasStateUpdate, HostRuntime, OutputRef, WorldRef};

/// Select a World's canvas with its stored extent and density.
pub(crate) trait CanvasTestHost {
    /// Queue the canvas state for the next mutation boundary and return the
    /// World's canvas output.
    fn canvas_output(&mut self, world: WorldRef, extent: [f32; 2], density: f32) -> OutputRef;
}

impl CanvasTestHost for HostRuntime {
    fn canvas_output(&mut self, world: WorldRef, extent: [f32; 2], density: f32) -> OutputRef {
        self.world_mut(world.id())
            .unwrap()
            .enqueue_canvas_state_update(CanvasStateUpdate {
                extent: Some(extent),
                units_per_metre: Some(density),
            })
            .unwrap();
        OutputRef::canvas(world)
    }
}
