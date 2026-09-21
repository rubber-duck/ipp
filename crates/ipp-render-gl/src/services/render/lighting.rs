//! Shared direct-light preparation; graphics handles remain in the device.

use crate::RenderError;
use ipp_core::{WorldContext, components::Camera, systems::camera};

/// Shader capacity for each individual draw. Scene candidate counts are unrestricted.
pub const MAX_LIGHTS: usize = 8;

pub(crate) const SHADOW_TILE_SIZE: u32 = 1024;

/// Context-independent frame uniforms. Four vec4s encode each punctual light.
#[derive(Clone)]
pub struct RenderLightingFrame {
    /// Perspective eye (w=0), or camera-facing orthographic direction (w=1).
    pub camera: [f32; 4],
    /// Uniform ambient fill from the World render state.
    pub ambient: [f32; 3],
    /// Position/kind, direction/inner cosine, RGB intensity/range,
    /// and outer cosine/projected emitter radius/shadow near/shadow far.
    pub lights: [f32; MAX_LIGHTS * 16],
    /// Number of initialized light records.
    pub count: i32,
    /// Number of active shadow spotlights, packed into stable atlas slots.
    pub shadow_count: u32,
    /// Projection * inverse pose for each light, in GL clip space.
    pub shadow_matrices: [f32; MAX_LIGHTS * 16],
    /// Per light: atlas slot (-1 disables), bias, inverse tile size, atlas grid size.
    pub shadow_settings: [f32; MAX_LIGHTS * 4],
}

impl RenderLightingFrame {
    pub(crate) fn prepare(
        world: &WorldContext<'_>,
        lights: &[(ipp_core::EntityId, [f32; 16], ipp_core::components::Light)],
    ) -> Result<Self, RenderError> {
        assert!(lights.len() <= MAX_LIGHTS, "per-draw shader capacity");
        let entity = world.active_camera().ok_or(RenderError::InvalidTransform)?;
        let orthographic = if ipp_core::render_buffer_reuse_enabled() {
            world
                .active_camera_component()
                .ok_or(RenderError::InvalidTransform)?
                .projection
                == 1
        } else {
            world
                .inspect(entity)
                .ok_or(RenderError::InvalidTransform)?
                .effective
                .iter()
                .any(|value| matches!(value, ipp_core::ComponentValue::Camera(c) if c.projection == 1))
        };
        let pose = world
            .world_matrix(entity)
            .map_err(|_| RenderError::InvalidTransform)?;
        let camera = if orthographic {
            let direction = unit([pose[8], pose[9], pose[10]])?;
            [direction[0], direction[1], direction[2], 1.0]
        } else {
            [pose[12], pose[13], pose[14], 0.0]
        };
        Self::prepare_with_camera(camera, world.render_state().ambient_light, lights)
    }

    pub(super) fn prepare_with_camera(
        camera: [f32; 4],
        ambient: [f32; 3],
        lights: &[(ipp_core::EntityId, [f32; 16], ipp_core::components::Light)],
    ) -> Result<Self, RenderError> {
        assert!(lights.len() <= MAX_LIGHTS, "per-draw shader capacity");
        let mut frame = Self::empty(camera, ambient);
        for (index, &(entity, model, light)) in lights.iter().enumerate() {
            frame.push(index, &PreparedLight::prepare(entity, model, light)?);
        }
        frame.finish();
        Ok(frame)
    }

    pub(super) fn empty(camera: [f32; 4], ambient: [f32; 3]) -> Self {
        Self {
            camera,
            ambient,
            lights: [0.0; MAX_LIGHTS * 16],
            count: 0,
            shadow_count: 0,
            shadow_matrices: [0.0; MAX_LIGHTS * 16],
            shadow_settings: std::array::from_fn(|i| {
                if i % 4 == 0 {
                    -1.0
                } else {
                    0.0
                }
            }),
        }
    }

    /// Rewrite retained uniform storage without moving a whole frame through a map insert.
    pub(super) fn reset(&mut self, camera: [f32; 4], ambient: [f32; 3]) {
        self.camera = camera;
        self.ambient = ambient;
        self.count = 0;
        self.shadow_count = 0;
    }

    pub(super) fn push(&mut self, index: usize, light: &PreparedLight) {
        self.lights[index * 16..(index + 1) * 16].copy_from_slice(&light.record);
        self.count += 1;
        self.shadow_settings[index * 4..index * 4 + 4].copy_from_slice(&[-1.0, 0.0, 0.0, 0.0]);
        if light.casts_shadow {
            self.shadow_matrices[index * 16..(index + 1) * 16]
                .copy_from_slice(&light.shadow_matrix);
            self.shadow_settings[index * 4] = self.shadow_count as f32;
            self.shadow_settings[index * 4 + 1] = light.shadow_bias;
            self.shadow_settings[index * 4 + 2] = 1.0 / SHADOW_TILE_SIZE as f32;
            self.shadow_count += 1;
        }
    }

    pub(super) fn finish(&mut self) {
        let grid = (self.shadow_count as f32).sqrt().ceil();
        for settings in self.shadow_settings[..self.count as usize * 4]
            .as_chunks_mut::<4>()
            .0
        {
            settings[3] = grid;
        }
    }
}

/// Object-independent light uniforms, evaluated at most once per light per submission.
/// Preparation remains lazy: an invalid light that no draw selects cannot fail the frame.
#[derive(Clone)]
pub(super) struct PreparedLight {
    record: [f32; 16],
    shadow_matrix: [f32; 16],
    shadow_bias: f32,
    casts_shadow: bool,
}

impl PreparedLight {
    pub(super) fn shadow_matrix(&self) -> [f32; 16] {
        self.shadow_matrix
    }

    pub(super) fn prepare(
        entity: ipp_core::EntityId,
        model: [f32; 16],
        light: ipp_core::components::Light,
    ) -> Result<Self, RenderError> {
        let forward = unit([-model[8], -model[9], -model[10]])?;
        let record = [
            model[12],
            model[13],
            model[14],
            light.kind as f32,
            forward[0],
            forward[1],
            forward[2],
            light.inner_cone.cos(),
            light.r * light.intensity,
            light.g * light.intensity,
            light.b * light.intensity,
            light.range,
            light.outer_cone.cos(),
            (f64::from(light.shadow_radius) / (2.0 * f64::from(light.outer_cone).tan()))
                .min(f64::from(f32::MAX)) as f32,
            light.shadow_near,
            light.range,
        ];
        let casts_shadow = cfg!(feature = "shadows") && light.cast_shadows;
        let shadow_matrix = if casts_shadow {
            camera::prepare_affine(
                entity,
                &Camera {
                    fov_y: 2.0 * light.outer_cone,
                    near: light.shadow_near,
                    far: light.range,
                    ..Camera::default()
                },
                &ipp_core::systems::geometry::GeometryShapeTransform::from_matrix(light_pose(
                    model,
                )?)
                .map_err(|_| RenderError::InvalidTransform)?,
                1,
                1,
            )
            .map_err(|_| RenderError::InvalidTransform)?
            .view_projection
        } else {
            [0.0; 16]
        };
        Ok(Self {
            record,
            shadow_matrix,
            shadow_bias: light.shadow_bias,
            casts_shadow,
        })
    }
}

fn unit(vector: [f32; 3]) -> Result<[f32; 3], RenderError> {
    let length = vector
        .iter()
        .map(|&v| f64::from(v).powi(2))
        .sum::<f64>()
        .sqrt();
    if !length.is_finite() || length == 0.0 {
        return Err(RenderError::InvalidTransform);
    }
    Ok(vector.map(|v| (f64::from(v) / length) as f32))
}

// Light range and cone angles stay in World units, including under scaled parents.
// Preserve the final position/forward while removing scale and shear from projection.
fn light_pose(model: [f32; 16]) -> Result<[f32; 16], RenderError> {
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let z = unit([model[8], model[9], model[10]])?;
    let x = unit(cross([model[4], model[5], model[6]], z))?;
    let y = cross(z, x);
    Ok([
        x[0], x[1], x[2], 0.0, y[0], y[1], y[2], 0.0, z[0], z[1], z[2], 0.0, model[12], model[13],
        model[14], 1.0,
    ])
}

#[cfg(test)]
mod tests {
    #[test]
    fn spotlight_projection_preserves_final_position_and_direction_without_scale_or_shear() {
        let model = [
            2.0, 0.0, 0.0, 0.0, 0.7, 3.0, 0.0, 0.0, 0.0, 0.0, 4.0, 0.0, 5.0, 6.0, 7.0, 1.0,
        ];
        let rigid = super::light_pose(model).unwrap();
        assert_eq!(&rigid[8..], &[0.0, 0.0, 1.0, 0.0, 5.0, 6.0, 7.0, 1.0]);
        for col in 0..3 {
            let length = (0..3).map(|row| rigid[col * 4 + row].powi(2)).sum::<f32>();
            assert!((length - 1.0).abs() < 1e-6);
        }
        assert!(
            (0..3)
                .map(|row| rigid[row] * rigid[4 + row])
                .sum::<f32>()
                .abs()
                < 1e-6
        );
    }
}
