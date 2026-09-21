//! Fixed-size solids and characteristic contours, independent of the renderer.

use std::f64::consts::{PI, TAU};

use crate::ErrorReason;

mod arrow;
#[cfg(feature = "builtin-assets")]
mod axis;
#[cfg(feature = "builtin-assets")]
mod cone;
mod plane;

const LONGITUDES: usize = 32;
const LATITUDES: usize = 16;
const CURVE_SEGMENTS: usize = 64;
const TUBE_SIDES: usize = 8;

type BuiltinMeshPoint = [f64; 3];

struct BuiltinMeshVertex {
    position: [f32; 3],
    color: [f32; 3],
    uv: [f32; 2],
    outward: BuiltinMeshPoint,
}

#[derive(Default)]
struct BuiltinMesh {
    normals: bool,
    vertices: Vec<BuiltinMeshVertex>,
    indices: Vec<u16>,
}

struct BuiltinPathPoint {
    center: BuiltinMeshPoint,
    radial: BuiltinMeshPoint,
    u: f64,
}

const X: BuiltinMeshPoint = [1.0, 0.0, 0.0];
const Y: BuiltinMeshPoint = [0.0, 1.0, 0.0];
const Z: BuiltinMeshPoint = [0.0, 0.0, 1.0];

mod builder;
use builder::circle_sample;
use builder::cylinder;
pub(super) use builder::debug_mesh;
#[cfg(feature = "builtin-assets")]
use builder::diameter;
#[cfg(feature = "builtin-assets")]
use builder::dimensions;
#[cfg(feature = "builtin-assets")]
pub(super) use builder::mesh;
#[cfg(feature = "builtin-assets")]
use builder::oriented_cylinder;
#[cfg(feature = "builtin-assets")]
use builder::ring_with_segments;
#[cfg(feature = "builtin-assets")]
use builder::validate_stroke;
