//! Exact per-program upload shadow state. Only active light slots are compared;
//! unchanged values retain GPU state, with no hash-based correctness decisions.

use crate::RenderLightingFrame;

pub(super) const CAMERA: u32 = 1;
pub(super) const AMBIENT: u32 = 2;
pub(super) const SURFACE: u32 = 4;
pub(super) const LIGHTS: u32 = 8;
pub(super) const COUNT: u32 = 16;
#[cfg(feature = "shadows")]
pub(super) const SHADOW_MATRICES: u32 = 32;
#[cfg(feature = "shadows")]
pub(super) const SHADOW_SETTINGS: u32 = 64;
#[cfg(feature = "shadows")]
pub(super) const SHADOW_SAMPLER: u32 = 128;

pub(super) struct RenderUniformCache {
    epoch: u64,
    valid: u32,
    camera: [f32; 4],
    ambient: [f32; 3],
    surface: [f32; 3],
    lights: [f32; 128],
    count: i32,
    #[cfg(feature = "shadows")]
    matrices: [f32; 128],
    #[cfg(feature = "shadows")]
    settings: [f32; 32],
    #[cfg(feature = "shadows")]
    shadow_length: usize,
}

impl Default for RenderUniformCache {
    fn default() -> Self {
        Self {
            epoch: 0,
            valid: 0,
            camera: [0.0; 4],
            ambient: [0.0; 3],
            surface: [0.0; 3],
            lights: [0.0; 128],
            count: 0,
            #[cfg(feature = "shadows")]
            matrices: [0.0; 128],
            #[cfg(feature = "shadows")]
            settings: [0.0; 32],
            #[cfg(feature = "shadows")]
            shadow_length: 0,
        }
    }
}

impl RenderUniformCache {
    fn begin(&mut self, epoch: u64) {
        if self.epoch != epoch {
            self.epoch = epoch;
            self.valid = 0;
        }
    }

    pub(super) fn lighting(
        &mut self,
        epoch: u64,
        surface: &[f32; 3],
        frame: &RenderLightingFrame,
    ) -> u32 {
        self.begin(epoch);
        let mut changed = 0;
        if self.valid & CAMERA == 0 || self.camera != frame.camera {
            self.camera = frame.camera;
            changed |= CAMERA;
        }
        if self.valid & AMBIENT == 0 || self.ambient != frame.ambient {
            self.ambient = frame.ambient;
            changed |= AMBIENT;
        }
        if self.valid & SURFACE == 0 || self.surface != *surface {
            self.surface = *surface;
            changed |= SURFACE;
        }
        let length = frame.count as usize * 16;
        if self.valid & LIGHTS == 0
            || self.count != frame.count
            || self.lights[..length] != frame.lights[..length]
        {
            self.lights[..length].copy_from_slice(&frame.lights[..length]);
            changed |= LIGHTS;
        }
        if self.valid & COUNT == 0 || self.count != frame.count {
            self.count = frame.count;
            changed |= COUNT;
        }
        self.valid |= changed;
        changed
    }

    #[cfg(feature = "shadows")]
    pub(super) fn shadows(&mut self, epoch: u64, frame: &RenderLightingFrame) -> u32 {
        self.begin(epoch);
        let mut changed = 0;
        let length = frame.count as usize;
        if frame.shadow_count > 0
            && (self.valid & SHADOW_MATRICES == 0
                || self.shadow_length != length
                || self.matrices[..length * 16] != frame.shadow_matrices[..length * 16])
        {
            self.matrices[..length * 16].copy_from_slice(&frame.shadow_matrices[..length * 16]);
            changed |= SHADOW_MATRICES;
        }
        if self.valid & SHADOW_SETTINGS == 0
            || self.shadow_length != length
            || self.settings[..length * 4] != frame.shadow_settings[..length * 4]
        {
            self.settings[..length * 4].copy_from_slice(&frame.shadow_settings[..length * 4]);
            changed |= SHADOW_SETTINGS;
        }
        if self.valid & SHADOW_SAMPLER == 0 {
            changed |= SHADOW_SAMPLER;
        }
        self.shadow_length = length;
        self.valid |= changed;
        changed
    }
}

#[cfg(test)]
#[path = "uniform_cache_tests.rs"]
mod tests;
