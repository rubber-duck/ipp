//! Renderer approximation of the completed headless Surface contract.
//!
//! A uniform dyadic grid avoids cracks between patches. Subdivision uses the
//! provider's conservative triangle error, at most half a selected image texel.
//! Work is capped at 128 cells per axis (16,641 vertices), with at most 256
//! independently depth-ordered patches. Exceeding the error/work bound makes
//! presentation unavailable rather than substituting an affine surface.

use crate::RenderError;
use ipp_core::systems::surface::Surface;
use std::ops::Range;

pub(super) const MAX_CELLS: usize = 128;
const PATCH_AXIS: usize = 16;

#[derive(Clone, Debug)]
pub(super) struct SurfaceMeshPatch {
    pub indices: Range<u32>,
    pub centre: [f64; 3],
}

pub(super) struct SurfaceMeshData {
    pub asset: ipp_core::MeshAsset,
    pub patches: Vec<SurfaceMeshPatch>,
    pub bytes: usize,
}

pub(super) fn tessellate(
    geometry: &dyn Surface,
    offset: f64,
    size: [u32; 2],
) -> Result<SurfaceMeshData, RenderError> {
    let extent = geometry.physical_extent();
    geometry
        .validate_offsets([offset, offset])
        .map_err(|_| RenderError::UnavailableOutput)?;
    if extent
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
        || size.contains(&0)
    {
        return Err(RenderError::UnavailableOutput);
    }
    let tolerance = (extent[0] / f64::from(size[0])).min(extent[1] / f64::from(size[1])) * 0.5;
    let mut cells = 1;
    loop {
        let step = extent.map(|value| value / cells as f64);
        let mut acceptable = true;
        for y in 0..cells {
            for x in 0..cells {
                let error = geometry
                    .approximation_error(
                        [
                            x as f64 * step[0],
                            y as f64 * step[1],
                            (x + 1) as f64 * step[0],
                            (y + 1) as f64 * step[1],
                        ],
                        offset,
                    )
                    .map_err(|_| RenderError::UnavailableOutput)?;
                if !error.is_finite() || error < 0.0 {
                    return Err(RenderError::UnavailableOutput);
                }
                acceptable &= error <= tolerance;
            }
        }
        if acceptable {
            break;
        }
        if cells == MAX_CELLS {
            return Err(RenderError::UnavailableOutput);
        }
        cells *= 2;
    }

    let mut positions = Vec::with_capacity((cells + 1).pow(2));
    let mut uvs = Vec::with_capacity(positions.capacity());
    for y in 0..=cells {
        for x in 0..=cells {
            let uv = [x as f64 / cells as f64, y as f64 / cells as f64];
            let point = geometry
                .sample([uv[0] * extent[0], uv[1] * extent[1]], offset)
                .map_err(|_| RenderError::UnavailableOutput)?
                .position
                .map(|value| value as f32);
            if point.iter().any(|value| !value.is_finite()) {
                return Err(RenderError::UnavailableOutput);
            }
            positions.push(point);
            uvs.push(uv.map(|value| value as f32));
        }
    }
    let mut indices = Vec::<u16>::with_capacity(cells * cells * 6);
    let mut patches = Vec::new();
    let patch_cells = (cells / PATCH_AXIS).max(1);
    for py in (0..cells).step_by(patch_cells) {
        for px in (0..cells).step_by(patch_cells) {
            let first = indices.len() as u32;
            for y in py..py + patch_cells {
                for x in px..px + patch_cells {
                    let top = (y * (cells + 1) + x) as u16;
                    let bottom = top + (cells + 1) as u16;
                    indices.extend_from_slice(&[top, bottom, top + 1, top + 1, bottom, bottom + 1]);
                }
            }
            let content = [
                (px as f64 + patch_cells as f64 * 0.5) / cells as f64 * extent[0],
                (py as f64 + patch_cells as f64 * 0.5) / cells as f64 * extent[1],
            ];
            let centre = geometry
                .sample(content, offset)
                .map_err(|_| RenderError::UnavailableOutput)?
                .position;
            patches.push(SurfaceMeshPatch {
                indices: first..indices.len() as u32,
                centre,
            });
        }
    }

    // The existing immutable mesh format supplies position and UV streams to
    // both compile-time devices. No runtime asset identity is created.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"IPPM");
    for word in [3, positions.len() as u32, indices.len() as u32, 2] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for (semantic, format, length) in [(0, 1, positions.len() * 12), (2, 2, uvs.len() * 8)] {
        bytes.extend_from_slice(&[semantic, format, 0, 0]);
        bytes.extend_from_slice(&(length as u32).to_le_bytes());
    }
    for point in positions {
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    for uv in uvs {
        for value in uv {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    for index in indices {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    let asset = ipp_core::MeshAsset::decode(&bytes).map_err(|_| RenderError::UnavailableOutput)?;
    let bytes = asset.vertex_bytes() + std::mem::size_of_val(asset.indices());
    Ok(SurfaceMeshData {
        asset,
        patches,
        bytes,
    })
}

/// Reduce raster quality, preserving exact provider geometry and the half-texel bound.
pub(super) fn select_quality(
    geometry: &dyn Surface,
    offsets: &[f64],
    mut size: [u32; 2],
) -> Result<([u32; 2], Vec<SurfaceMeshData>), RenderError> {
    loop {
        let meshes: Result<Vec<_>, _> = offsets
            .iter()
            .map(|offset| tessellate(geometry, *offset, size))
            .collect();
        match meshes {
            Ok(meshes) => return Ok((size, meshes)),
            Err(error) if size == [1, 1] => return Err(error),
            Err(_) => size = size.map(|value| (value / 2).max(1)),
        }
    }
}

#[cfg(test)]
#[path = "surface_mesh_tests.rs"]
mod tests;
