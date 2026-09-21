//! Renderer-owned packing for shared quadratic contours.

use ipp_core::services::asset_management::quadratic::{QuadraticContour, QuadraticSegment};

pub(super) const BAND_COUNT: usize = 16;

use super::device::SurfacePathDescriptor;

pub(super) struct SurfacePathAtlas {
    pub curves: Vec<[f32; 8]>,
    pub bands: Vec<[u32; 2]>,
    pub descriptors: Vec<SurfacePathDescriptor>,
}

pub(super) fn atlas<'a>(
    paths: impl IntoIterator<Item = ([f32; 4], &'a [QuadraticContour])>,
) -> SurfacePathAtlas {
    let mut curves = Vec::new();
    let mut bands = Vec::new();
    let mut descriptors = Vec::new();
    for (bounds, contours) in paths {
        let start = curves.len() as u32;
        curves.extend(pack(contours));
        let count = curves.len() as u32 - start;
        let band_offset = bands.len() as u32;
        bands.resize(bands.len() + BAND_COUNT * 2, [0; 2]);
        for (group, axis) in [1, 0].into_iter().enumerate() {
            let extent = bounds[axis + 2] - bounds[axis];
            for band in 0..BAND_COUNT {
                let list_offset = bands.len() as u32;
                let low = bounds[axis] + extent * band as f32 / BAND_COUNT as f32;
                let high = bounds[axis] + extent * (band + 1) as f32 / BAND_COUNT as f32;
                for curve in start..start + count {
                    let points = curves[curve as usize];
                    let minimum = points[axis].min(points[axis + 2]).min(points[axis + 4]);
                    let maximum = points[axis].max(points[axis + 2]).max(points[axis + 4]);
                    if maximum >= low && minimum <= high {
                        bands.push([curve, 0]);
                    }
                }
                bands[band_offset as usize + group * BAND_COUNT + band] =
                    [list_offset, bands.len() as u32 - list_offset];
            }
        }
        descriptors.push(SurfacePathDescriptor::new([start, count], band_offset));
    }
    SurfacePathAtlas {
        curves,
        bands,
        descriptors,
    }
}

/// GPU segment layout shared by the WebGL and GLES surface devices.
///
/// Lines duplicate their endpoint as the quadratic control and use kind zero,
/// following the Slug reference packing. Quadratics use kind one. Keeping one
/// fixed layout makes provider output independent of the compile-time device
/// while leaving it outside persistent assets.
pub(super) fn pack(contours: &[QuadraticContour]) -> Vec<[f32; 8]> {
    let count = contours.iter().map(|contour| contour.segments.len()).sum();
    let mut packed = Vec::with_capacity(count);
    for contour in contours {
        let mut start = contour.start;
        for segment in &contour.segments {
            let (control, end, kind) = match *segment {
                QuadraticSegment::Line {
                    to,
                } => (to, to, 0.0),
                QuadraticSegment::Quadratic {
                    control,
                    to,
                } => (control, to, 1.0),
            };
            packed.push([
                start[0], start[1], control[0], control[1], end[0], end[1], kind, 0.0,
            ]);
            start = end;
        }
        if start != contour.start {
            let to = contour.start;
            packed.push([start[0], start[1], to[0], to[1], to[0], to[1], 0.0, 0.0]);
        }
    }
    packed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packing_preserves_contour_order_and_explicit_curve_kind() {
        let contours = [QuadraticContour {
            start: [-1.0, 2.0],
            segments: vec![
                QuadraticSegment::Line {
                    to: [3.0, 4.0],
                },
                QuadraticSegment::Quadratic {
                    control: [5.0, 6.0],
                    to: [-1.0, 2.0],
                },
            ],
        }];
        assert_eq!(
            pack(&contours),
            vec![
                [-1.0, 2.0, 3.0, 4.0, 3.0, 4.0, 0.0, 0.0],
                [3.0, 4.0, 5.0, 6.0, -1.0, 2.0, 1.0, 0.0],
            ]
        );
    }

    #[test]
    fn packing_closes_implicit_contour_with_linear_quadratic() {
        let packed = pack(&[QuadraticContour {
            start: [0.0, 0.0],
            segments: vec![QuadraticSegment::Line {
                to: [2.0, 0.0],
            }],
        }]);
        assert_eq!(packed[1], [2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn atlas_bands_reference_only_spatially_overlapping_curves() {
        let contours = [QuadraticContour {
            start: [0.0, 0.0],
            segments: vec![QuadraticSegment::Line {
                to: [1.0, 0.0],
            }],
        }];
        let atlas = atlas([([0.0, 0.0, 1.0, 1.0], contours.as_slice())]);
        let descriptor = atlas.descriptors[0];
        let bottom = atlas.bands[descriptor.band_offset as usize];
        let top = atlas.bands[descriptor.band_offset as usize + BAND_COUNT - 1];
        assert!(bottom[1] > 0);
        assert_eq!(top[1], 0);
    }
}
