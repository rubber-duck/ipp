use super::*;

fn layering(spacing: f32, eye: CanvasLayerEye) -> CanvasLayering {
    CanvasLayering {
        spacing,
        eye,
    }
}

fn draw_order(layering: CanvasLayering, offsets: &[f64]) -> Vec<usize> {
    let mut order: Vec<_> = (0..offsets.len()).collect();
    if layering.spacing != 0.0 {
        order.sort_by(|left, right| {
            layering
                .view_depth(offsets[*right])
                .total_cmp(&layering.view_depth(offsets[*left]))
        });
    }
    order
}

#[test]
fn physical_planes_draw_farthest_first_from_any_side() {
    use CanvasLayerEye::{Direction, Point};

    let offsets = [0.0, 0.75, 2.0];
    assert_eq!(draw_order(layering(1.0, Point(5.0)), &offsets), [0, 1, 2]);
    assert_eq!(draw_order(layering(1.0, Point(-5.0)), &offsets), [2, 1, 0]);
    assert_eq!(draw_order(layering(1.0, Point(1.2)), &offsets), [0, 2, 1]);
    assert_eq!(draw_order(layering(-1.0, Point(5.0)), &offsets), [2, 1, 0]);
    assert_eq!(
        draw_order(layering(1.0, Direction(-0.5)), &offsets),
        [0, 1, 2]
    );
    assert_eq!(
        draw_order(layering(1.0, Direction(0.5)), &offsets),
        [2, 1, 0]
    );
}

#[test]
fn physical_ids_do_not_reorder_coincident_logical_entries() {
    use CanvasLayerEye::{Direction, Point};

    // Logical paint order visits plane1, plane0, plane1. Flattening must keep
    // the middle entry between the two paints on the same physical plane.
    let offsets = [2.0, 0.0, 2.0];
    assert_eq!(draw_order(CanvasLayering::FLAT, &offsets), [0, 1, 2]);
    assert_eq!(
        draw_order(layering(1.0, Direction(0.0)), &offsets),
        [0, 1, 2]
    );
    assert_eq!(draw_order(layering(1.0, Point(1.0)), &offsets), [0, 1, 2]);
}

#[test]
fn a_fractional_position_translates_content_by_the_spacing() {
    let mvp: [f32; 16] = std::array::from_fn(|index| index as f32 + 1.0);
    assert_eq!(CanvasLayering::FLAT.layer_mvp(&mvp, 3.75), mvp);
    let layered = layering(0.5, CanvasLayerEye::Point(1.0));
    assert_eq!(layered.layer_mvp(&mvp, 0.0), mvp);
    let raised = layered.layer_mvp(&mvp, 1.5);
    assert_eq!(raised[..12], mvp[..12]);
    assert_eq!(raised[12..], [19.75, 21.5, 23.25, 25.0]);
}

#[derive(Debug)]
struct ShiftedRotatedSurface(ipp_core::FlatSurface);

impl ipp_core::systems::surface::Surface for ShiftedRotatedSurface {
    fn physical_extent(&self) -> [f64; 2] {
        self.0.physical_extent()
    }

    fn layer_spacing(&self) -> f32 {
        self.0.layer_spacing()
    }

    fn sample(
        &self,
        content: [f64; 2],
        offset: f64,
    ) -> Result<ipp_core::systems::surface::SurfaceSample, ipp_core::ErrorReason> {
        let sample = self.0.sample(content, offset)?;
        let [x, y, z] = sample.position;
        Ok(ipp_core::systems::surface::SurfaceSample {
            position: [2.0 + z, 3.0 + y, 4.0 - x],
            front_normal: [1.0, 0.0, 0.0],
        })
    }

    fn ray_intersections(
        &self,
        ray: &ipp_core::systems::geometry::GeometryRay,
        offset: f64,
        domain: ipp_core::systems::surface::SurfaceDomain,
    ) -> Result<Vec<ipp_core::systems::surface::SurfaceIntersection>, ipp_core::ErrorReason> {
        let local = ipp_core::systems::geometry::GeometryRay {
            origin: [
                4.0 - ray.origin[2],
                ray.origin[1] - 3.0,
                ray.origin[0] - 2.0,
            ],
            direction: [-ray.direction[2], ray.direction[1], ray.direction[0]],
        };
        let mut hits = self.0.ray_intersections(&local, offset, domain)?;
        for hit in &mut hits {
            hit.front_normal = [1.0, 0.0, 0.0];
        }
        Ok(hits)
    }

    fn validate_offsets(&self, offsets: [f64; 2]) -> Result<(), ipp_core::ErrorReason> {
        self.0.validate_offsets(offsets)
    }

    fn bounds(
        &self,
        offsets: [f64; 2],
    ) -> Result<ipp_core::systems::geometry::GeometryShape, ipp_core::ErrorReason> {
        let extent = self.physical_extent();
        Ok(ipp_core::systems::geometry::GeometryShape::Box {
            min: [
                2.0 + offsets[0],
                3.0 - extent[1] * 0.5,
                4.0 - extent[0] * 0.5,
            ],
            max: [
                2.0 + offsets[1],
                3.0 + extent[1] * 0.5,
                4.0 + extent[0] * 0.5,
            ],
        })
    }

    fn approximation_error(
        &self,
        patch: [f64; 4],
        offset: f64,
    ) -> Result<f64, ipp_core::ErrorReason> {
        self.0.approximation_error(patch, offset)
    }

    fn exact_affine(&self, offset: f64) -> Option<[f64; 16]> {
        let [width, height] = self.physical_extent();
        Some([
            0.0,
            0.0,
            -1.0,
            0.0,
            0.0,
            -1.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            2.0 + offset,
            3.0 + height * 0.5,
            4.0 + width * 0.5,
            1.0,
        ])
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn equivalent(&self, other: &dyn ipp_core::systems::surface::Surface) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|v| v.0 == self.0)
    }
}

#[test]
fn generic_affine_provider_preserves_shift_rotation_scale_and_normal_offsets() {
    use ipp_core::systems::surface::Surface;
    let provider = ShiftedRotatedSurface(ipp_core::FlatSurface {
        width: 4.0,
        height: 2.0,
        layer_spacing: 0.25,
    });
    let mapping = affine_matrix(&provider).unwrap();
    let content = camera::multiply(mapping, plane_matrix([0.0; 2], [0.01, 0.01]).unwrap());
    for logical in [[0.0, 0.0], [400.0, 200.0], [130.0, 80.0]] {
        for coordinate in [0.0, 1.375, 2.0] {
            let mvp =
                layering(0.25, CanvasLayerEye::Direction(-1.0)).layer_mvp(&content, coordinate);
            let actual: [f64; 3] = std::array::from_fn(|i| {
                f64::from(mvp[i]) * logical[0]
                    + f64::from(mvp[4 + i]) * logical[1]
                    + f64::from(mvp[12 + i])
            });
            let expected = provider
                .sample(logical.map(|v| v * 0.01), coordinate * 0.25)
                .unwrap()
                .position;
            for (a, e) in actual.into_iter().zip(expected) {
                assert!((a - e).abs() < 1e-6);
            }
        }
    }
}
