use super::{RenderReadAccess, RenderSystem};
use crate::{ErrorReason, RenderState, RenderStatePatch};

impl RenderSystem {
    pub(in crate::world) fn update_render_state(
        &mut self,
        patch: RenderStatePatch,
    ) -> Result<Option<RenderStatePatch>, ErrorReason> {
        if patch.debug_geometry_color.is_some_and(|color| {
            color
                .iter()
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
        }) {
            return Err(ErrorReason::InvalidValue);
        }

        if patch
            .ambient_light
            .is_some_and(|color| color.iter().any(|value| !value.is_finite() || *value < 0.0))
        {
            return Err(ErrorReason::InvalidValue);
        }

        let mut changes = RenderStatePatch::default();
        if let Some(value) = patch.show_all_debug_geometries
            && value != self.state.render_state.show_all_debug_geometries
        {
            self.state.render_state.show_all_debug_geometries = value;
            changes.show_all_debug_geometries = Some(value);
        }
        if let Some(value) = patch.debug_geometry_color
            && value != self.state.render_state.debug_geometry_color
        {
            self.state.render_state.debug_geometry_color = value;
            changes.debug_geometry_color = Some(value);
        }
        if let Some(value) = patch.ambient_light
            && value != self.state.render_state.ambient_light
        {
            self.state.render_state.ambient_light = value;
            changes.ambient_light = Some(value);
        }
        if changes == RenderStatePatch::default() {
            return Ok(None);
        }
        crate::diagnostic!(Debug, "[IPP core] render_state.update changes={changes:?}");
        Ok(Some(changes))
    }
}

impl RenderReadAccess<'_> {
    pub fn render_state(&self) -> RenderState {
        self.render.render_state
    }
}

impl crate::WorldContext<'_> {
    /// Queue a sparse settings update alongside batches and camera selection.
    pub fn enqueue_render_state_update(
        &mut self,
        patch: RenderStatePatch,
    ) -> Result<(), ErrorReason> {
        self.enqueue_system_command(RenderSystem::ID, 0, patch)
    }

    /// Current committed settings. Fresh worlds start with defaults.
    pub fn render_state(&self) -> RenderState {
        self.render_read().render_state()
    }
}
