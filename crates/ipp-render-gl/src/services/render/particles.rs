//! View-specific packing of already evaluated particles. Simulation remains in core.
use super::assets::{GlMeshData, SharedRenderDevice};
use crate::{RenderDevice, RenderError};
use ipp_core::systems::geometry::GeometryBounds;

pub(super) fn quad_asset() -> ipp_core::MeshAsset {
    let mut bytes = b"IPPM".to_vec();
    for word in [2u32, 4, 6] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for [x, y, u, v] in [
        [-0.5f32, -0.5, 0.0, 1.0],
        [0.5, -0.5, 1.0, 1.0],
        [0.5, 0.5, 1.0, 0.0],
        [-0.5, 0.5, 0.0, 0.0],
    ] {
        for f in [x, y, 0.0, 1.0, 1.0, 1.0, u, v] {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
    }
    for i in [0u16, 1, 2, 0, 2, 3] {
        bytes.extend_from_slice(&i.to_le_bytes());
    }
    ipp_core::MeshAsset::decode(&bytes)
        .expect("private particle quad")
        .0
}

pub(super) fn quad<D: RenderDevice>(
    device: SharedRenderDevice<D>,
) -> Result<GlMeshData<D>, RenderError> {
    GlMeshData::private_mesh(device, quad_asset())
}

#[cfg(feature = "shadows")]
pub(super) fn identity() -> [f32; 16] {
    [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ]
}

pub(super) fn instance(item: &ipp_core::RenderItem, camera: [f32; 16]) -> [f32; 20] {
    let p = item.particle.expect("particle instance");
    let mut model = item.model;
    if p.sprite {
        let scale = |col: usize| {
            (0..3)
                .map(|i| model[col * 4 + i].powi(2))
                .sum::<f32>()
                .sqrt()
        };
        let sx = scale(0);
        let sy = scale(1);
        let mut right = [camera[0], camera[1], camera[2]];
        let mut up = [camera[4], camera[5], camera[6]];
        let normalize = |v: [f32; 3]| {
            let length = v
                .iter()
                .map(|v| v * v)
                .sum::<f32>()
                .sqrt()
                .max(f32::MIN_POSITIVE);
            v.map(|v| v / length)
        };
        right = normalize(right);
        up = normalize(up);
        let dot = |v: [f32; 3], b: [f32; 3]| v.iter().zip(b).map(|(v, b)| v * b).sum::<f32>();
        let angle = if p.velocity_aligned {
            let x = dot(p.velocity, right);
            let y = dot(p.velocity, up);
            if x * x + y * y > 1e-12 {
                -x.atan2(y)
            } else {
                0.0
            }
        } else {
            item.transform.qz.atan2(item.transform.qw) * 2.0
        };
        let (s, c) = angle.sin_cos();
        for i in 0..3 {
            model[i] = (right[i] * c + up[i] * s) * sx;
            model[4 + i] = (-right[i] * s + up[i] * c) * sy;
        }
        model[8] = right[1] * up[2] - right[2] * up[1];
        model[9] = right[2] * up[0] - right[0] * up[2];
        model[10] = right[0] * up[1] - right[1] * up[0];
    }
    let mut words = [0.0; 20];
    words[..16].copy_from_slice(&model);
    words[16] = p.opacity;
    words
}

pub(super) fn visible(
    world: &ipp_core::WorldContext<'_>,
    item: &ipp_core::RenderItem,
    instances: &[[f32; 20]],
    planes: &[ipp_core::systems::geometry::GeometryPlane; 6],
) -> bool {
    let Some(shape) = world.particle_bounding_geometry(item.entity) else {
        return true;
    };
    if shape.intersects_frustum(planes) {
        return true;
    }
    #[cfg(feature = "mesh-poses")]
    if item.pose.is_some() {
        return true;
    }
    #[cfg(feature = "skeletal-animation")]
    if item.skinned {
        return true;
    }
    let (min, max) = if item.particle.is_some_and(|p| p.sprite) {
        ([-0.5, -0.5, 0.0], [0.5, 0.5, 0.0])
    } else {
        let Some(mesh) = world.mesh_metadata(item.mesh) else {
            return true;
        };
        mesh.bounds()
    };
    let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for model in instances {
        for corner in 0..8 {
            let local: [f64; 3] = std::array::from_fn(|i| {
                f64::from(if corner & (1 << i) == 0 {
                    min[i]
                } else {
                    max[i]
                })
            });
            for i in 0..3 {
                let value = f64::from(model[i]) * local[0]
                    + f64::from(model[i + 4]) * local[1]
                    + f64::from(model[i + 8]) * local[2]
                    + f64::from(model[i + 12]);
                bounds[0][i] = bounds[0][i].min(value);
                bounds[1][i] = bounds[1][i].max(value);
            }
        }
    }
    !shape.encloses_box(bounds)
}
