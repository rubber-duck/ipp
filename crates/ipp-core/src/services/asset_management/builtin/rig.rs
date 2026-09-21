//! Original analytic two-joint strip fixture. No imported model or external data.

use crate::{ErrorReason, services::asset_management::AssetTypeId};

/// Deterministic rig sources. Exact URIs have no mutable parameters or variants.
pub fn rig(kind: AssetTypeId, uri: &str) -> Result<Vec<u8>, ErrorReason> {
    let mut bytes = Vec::new();
    match (kind, uri) {
        (crate::SKELETON_TYPE, "ipp://skeleton/rig-strip") => {
            header(&mut bytes, b"IPPS");
            for (parent, y) in [(u32::MAX, 0.0), (0, 1.0)] {
                bytes.extend(parent.to_le_bytes());
                trs(&mut bytes, y, 0.0);
            }
        }
        (crate::POSE_TYPE, "ipp://pose/rig-strip-bent") => {
            header(&mut bytes, b"IPPP");
            trs(&mut bytes, 0.0, 0.0);
            trs(&mut bytes, 1.0, std::f32::consts::FRAC_PI_2);
        }

        (crate::SKIN_TYPE, "ipp://skin/rig-strip") => {
            header(&mut bytes, b"IPPB");
            // Deliberately reverse the palette mapping to exercise indirection.
            for joint in [1u32, 0] {
                bytes.extend(joint.to_le_bytes());
                floats(
                    &mut bytes,
                    &[
                        1.0,
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                        1.0,
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                        1.0,
                        0.0,
                        0.0,
                        -(joint as f32),
                        0.0,
                        1.0,
                    ],
                );
            }
        }

        (crate::MESH_TYPE, "ipp://mesh/rig-strip") => return Ok(mesh()),
        _ => return Err(ErrorReason::InvalidAsset),
    }
    Ok(bytes)
}

fn header(bytes: &mut Vec<u8>, magic: &[u8; 4]) {
    bytes.extend(magic);
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(2u32.to_le_bytes());
}

fn trs(bytes: &mut Vec<u8>, y: f32, angle: f32) {
    let (sine, cosine) = (angle * 0.5).sin_cos();
    floats(bytes, &[0.0, y, 0.0, 0.0, 0.0, sine, cosine, 1.0, 1.0, 1.0]);
}

fn floats(bytes: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        bytes.extend(value.to_le_bytes());
    }
}

fn mesh() -> Vec<u8> {
    // Nine rows, width .5m and height 2m in XY. Red lower half, blue upper.
    let mut bytes = b"IPPM".to_vec();
    for n in [3u32, 18, 48, 4] {
        bytes.extend(n.to_le_bytes());
    }
    for (semantic, format, width) in [(0, 1, 12u32), (1, 1, 12), (5, 4, 4), (6, 5, 16)] {
        bytes.extend([semantic, format, 0, 0]);
        bytes.extend((18 * width).to_le_bytes());
    }
    for row in 0..9 {
        for x in [-0.25, 0.25] {
            floats(&mut bytes, &[x, row as f32 * 0.25, 0.0]);
        }
    }
    for row in 0..9 {
        for _ in 0..2 {
            let t = row as f32 / 8.0;
            floats(&mut bytes, &[1.0 - t, 0.15, t]);
        }
    }
    for _ in 0..18 {
        bytes.extend([1, 0, 0, 0]);
    }
    for row in 0..9 {
        for _ in 0..2 {
            let upper = ((row as f32 * 0.25 - 0.5) / 1.0).clamp(0.0, 1.0);
            floats(&mut bytes, &[1.0 - upper, upper, 0.0, 0.0]);
        }
    }
    for row in 0..8u16 {
        let a = row * 2;
        for index in [a, a + 1, a + 2, a + 1, a + 3, a + 2] {
            bytes.extend(index.to_le_bytes());
        }
    }
    bytes
}
