//! Typed session-scoped rendering settings, applied at the mutation boundary.

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
