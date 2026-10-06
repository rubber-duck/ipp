//! Typed session-scoped rendering settings, applied at the mutation boundary.

use super::{RenderReadAccess, RenderSystem};
use crate::ErrorReason;

/// Global settings copied by renderers without mutating authored components.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderState {
    /// Reveal every effective debug shape, preserving individual visibility.
    pub show_all_debug_geometries: bool,
    /// Default linear RGB color for debug shapes without a component override.
    pub debug_geometry_color: [f32; 3],
    /// Uniform linear RGB fill added to PBR base color; zero disables ambient light.
    pub ambient_light: [f32; 3],
}

impl Default for RenderState {
    fn default() -> Self {
        Self {
            show_all_debug_geometries: false,
            debug_geometry_color: [1.0, 0.8, 0.0],
            ambient_light: [0.0; 3],
        }
    }
}

/// Sparse authored update. Omitted settings retain their current values.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderStatePatch {
    /// Optional global visibility replacement.
    pub show_all_debug_geometries: Option<bool>,
    /// Optional linear RGB replacement, with three finite channels in 0..=1.
    pub debug_geometry_color: Option<[f32; 3]>,
    /// Optional finite, nonnegative linear RGB ambient fill. HDR values are valid.
    pub ambient_light: Option<[f32; 3]>,
}

/// A sparse system transition emitted after the frame commits, without correlation.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderStateChange {
    /// Frame in which the settings changed.
    pub tick: u64,
    /// Only settings whose values changed in this committed transition.
    pub changes: RenderStatePatch,
}

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
    pub fn render_state(&self) -> Result<RenderState, ErrorReason> {
        Ok(self
            .render_read()
            .ok_or(ErrorReason::UnsupportedDependency)?
            .render_state())
    }
}
