//! View-dependent placement of particles already evaluated by the World.

use ipp_core::RenderItem;

use crate::device::{Device, MeshBuffers, MeshVertex};

pub(super) fn quad(device: &Device) -> Option<MeshBuffers> {
    let vertices = [
        [-0.5, -0.5, 0.0, 1.0],
        [0.5, -0.5, 1.0, 1.0],
        [0.5, 0.5, 1.0, 0.0],
        [-0.5, 0.5, 0.0, 0.0],
    ]
    .map(|[x, y, u, v]| MeshVertex {
        position: [x, y, 0.0, 1.0],
        color_weight: [1.0; 4],
        uv: [u, v, 0.0, 0.0],
        joints: [0; 4],
        joint_weights: [1.0, 0.0, 0.0, 0.0],
        normal: [0.0, 0.0, 1.0, 0.0],
    });
    device.mesh(&vertices, &[0, 1, 2, 0, 2, 3])
}

pub(super) fn billboard(
    item: &RenderItem,
    camera: [f32; 16],
    placement: [f32; 16],
    mut model: [f32; 16],
) -> Option<[f32; 16]> {
    let particle = item.particle.filter(|p| p.sprite)?;
    let right = normalized([camera[0], camera[1], camera[2]]);
    let up = normalized([camera[4], camera[5], camera[6]]);
    let sx = dot(
        [model[0], model[1], model[2]],
        [model[0], model[1], model[2]],
    )
    .sqrt();
    let sy = dot(
        [model[4], model[5], model[6]],
        [model[4], model[5], model[6]],
    )
    .sqrt();

    let angle = if particle.velocity_aligned {
        let velocity = std::array::from_fn(|i| {
            placement[i] * particle.velocity[0]
                + placement[i + 4] * particle.velocity[1]
                + placement[i + 8] * particle.velocity[2]
        });
        let x = dot(velocity, right);
        let y = dot(velocity, up);
        if x * x + y * y > 1e-12 {
            -x.atan2(y)
        } else {
            0.0
        }
    } else {
        2.0 * item.transform.qz.atan2(item.transform.qw)
    };
    let (sin, cos) = angle.sin_cos();
    for i in 0..3 {
        model[i] = (right[i] * cos + up[i] * sin) * sx;
        model[4 + i] = (up[i] * cos - right[i] * sin) * sy;
    }
    model[8] = right[1] * up[2] - right[2] * up[1];
    model[9] = right[2] * up[0] - right[0] * up[2];
    model[10] = right[0] * up[1] - right[1] * up[0];
    Some(model)
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}

fn normalized(v: [f32; 3]) -> [f32; 3] {
    let length = dot(v, v).sqrt().max(f32::MIN_POSITIVE);
    v.map(|value| value / length)
}
