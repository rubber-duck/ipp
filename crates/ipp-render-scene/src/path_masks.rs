//! Bounded screen-pixel addressing for exact projected path coverage.
use crate::canvas::CanvasVertex;

/// A pass may retain 4 MiB of coverage. Sample addresses fit exactly in paint records.
pub const MAX_PIXELS: u32 = 1 << 20;

/// Reserve a disjoint rectangle for one projected primitive, padded for raster rounding.
pub fn reserve(vertices: &[CanvasVertex], size: [u32; 2], used: &mut u32) -> Option<[f32; 4]> {
    let mut lo = size;
    let mut hi = [0; 2];
    for v in vertices {
        let [x, y, _, w] = v.clip;
        if w <= 0. || !w.is_finite() {
            return None;
        }

        let pos = [
            (x / w + 1.) * 0.5 * size[0] as f32,
            (1. - y / w) * 0.5 * size[1] as f32,
        ];
        for i in 0..2 {
            if !pos[i].is_finite() {
                return None;
            }
            // Include viewport rounding at rasterization and clip partially visible covers.
            lo[i] = lo[i].min((pos[i].floor() as u32).saturating_sub(1));
            hi[i] = hi[i].max((pos[i].ceil() as u32).saturating_add(1).min(size[i]));
        }
    }

    let width = hi[0].checked_sub(lo[0])?;
    let height = hi[1].checked_sub(lo[1])?;
    let count = width.checked_mul(height)?;
    let start = *used;
    if count == 0 || start.checked_add(count)? > MAX_PIXELS {
        return None;
    }

    *used += count;
    Some([lo[0] as f32, lo[1] as f32, width as f32, start as f32])
}

#[cfg(test)]
#[path = "path_masks_tests.rs"]
mod tests;
