//! Compact Camera-facing sprite instances. Orientation is expanded by the Metal vertex shader.
use ipp_core::{RenderItem, systems::particles::ParticleRenderData};

// Bound a single transient upload to 384 KiB. Larger streams continue in order.
pub(super) const MAX_BATCH: usize = 8192;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::NoUninit)]
pub(super) struct SpriteView {
    projection: [f32; 16],
    right: [f32; 4],
    up: [f32; 4],
}

impl SpriteView {
    pub fn new(projection: [f32; 16], camera: [f32; 16]) -> Self {
        let axis = |v: &[f32]| {
            let inv = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2])
                .sqrt()
                .max(f32::MIN_POSITIVE)
                .recip();
            [v[0] * inv, v[1] * inv, v[2] * inv, 0.]
        };

        Self {
            projection,
            right: axis(&camera[..3]),
            up: axis(&camera[4..7]),
        }
    }

    pub fn instance(
        &self,
        item: &RenderItem,
        particle: ParticleRenderData,
        placement: [f32; 16],
        model: [f32; 16],
    ) -> Sprite {
        let scale = |i| {
            let v: &[f32] = &model[i..i + 3];
            (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
        };

        let (x, y) = if particle.velocity_aligned {
            let v: [f32; 3] = std::array::from_fn(|i| {
                placement[i] * particle.velocity[0]
                    + placement[i + 4] * particle.velocity[1]
                    + placement[i + 8] * particle.velocity[2]
            });
            let dot = |axis: [f32; 4]| v[0] * axis[0] + v[1] * axis[1] + v[2] * axis[2];
            (-dot(self.right), dot(self.up))
        } else {
            (item.transform.qz, item.transform.qw)
        };

        Sprite {
            center_scale: [model[12], model[13], model[14], scale(0)],
            rotation: [x, y, scale(4), f32::from(particle.velocity_aligned)],
            color: [
                item.material.r,
                item.material.g,
                item.material.b,
                particle.opacity,
            ],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::NoUninit)]
pub(super) struct Sprite {
    center_scale: [f32; 4],
    rotation: [f32; 4],
    color: [f32; 4],
}
