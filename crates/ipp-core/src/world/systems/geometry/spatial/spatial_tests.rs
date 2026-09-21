use super::*;
use crate::{
    EntityId,
    systems::geometry::{GeometryBounds, GeometryEnclosure, GeometryPlane, GeometryShape},
};

fn frustum(center: [f64; 3], half: f64) -> [GeometryPlane; 6] {
    std::array::from_fn(|i| {
        let axis = i / 2;
        let sign = if i % 2 == 0 {
            1.0
        } else {
            -1.0
        };
        let mut normal = [0.0; 3];
        normal[axis] = sign;
        GeometryPlane {
            normal,
            offset: half - sign * center[axis],
        }
    })
}

#[test]
fn masked_batches_match_independent_queries_after_refits_and_rebuilds() {
    for backend in [GeometrySpatialBackend::Bvh, GeometrySpatialBackend::Flat] {
        let mut index = GeometrySpatialIndex::default();
        index.set_backend(backend);
        let queries: Vec<_> = (0..129)
            .map(|i| frustum([(i % 17) as f64 * 3.0, (i % 11) as f64 * 2.0, 0.0], 2.5))
            .collect();
        let mut results = GeometryQueryResults::default();
        let mut scratch = GeometryQueryScratch::default();
        for frame in 0..70 {
            if frame == 35 {
                index.invalidate();
            }
            for i in 0..257 {
                let entity = EntityId::from_bits(
                    i as u64
                        | if frame >= 35 {
                            1u64 << 32
                        } else {
                            0
                        },
                );
                let x = (i % 31) as f64 * 1.5 + (frame as f64 * 0.04).sin() * (i % 5) as f64;
                let y = (i % 13) as f64 * 2.0;
                let bound = GeometryEnclosure::new([[x, y, -0.5], [x + 0.8, y + 0.6, 0.5]]);
                index.publish(GeometryPreparedBounds {
                    entity,
                    visual: bound,
                    culling: if i % 37 == 0 {
                        None
                    } else {
                        bound
                    },
                });
            }
            {
                index.finish();
                index.query_frustums(&queries, &mut results, &mut scratch);
                for row in index.rows.iter().flatten() {
                    for (query, planes) in queries.iter().enumerate() {
                        let expected = row.culling.is_none_or(|b| {
                            GeometryShape::Box {
                                min: b.bounds[0],
                                max: b.bounds[1],
                            }
                            .intersects_frustum(planes)
                        });
                        assert_eq!(
                            results.matches(row.entity, query),
                            expected,
                            "frame {frame}, query {query}, backend {backend:?}"
                        );
                    }
                }
            }
        }
        index.invalidate();
        index.finish();
        index.query_frustums(&queries[..1], &mut results, &mut scratch);
        assert!(
            results.matches(EntityId::from_bits(123), 0),
            "absent bounds are unknown"
        );
        index.query_frustums(&[], &mut results, &mut scratch);
    }
}

#[test]
fn sibling_masks_and_boundary_contacts_are_preserved() {
    let mut index = GeometrySpatialIndex::default();
    for (i, x) in [-100.0, -99.0, 99.0, 100.0].into_iter().enumerate() {
        let bound = GeometryEnclosure::new([[x, 0.0, 0.0], [x, 0.0, 0.0]]);
        index.publish(GeometryPreparedBounds {
            entity: EntityId::from_bits(i as u64),
            visual: bound,
            culling: bound,
        });
    }
    index.finish();
    let mut results = GeometryQueryResults::default();
    let mut scratch = GeometryQueryScratch::default();
    index.query_frustums(
        &[
            frustum([-100.0, 0.0, 0.0], 1.0),
            frustum([100.0, 0.0, 0.0], 1.0),
        ],
        &mut results,
        &mut scratch,
    );
    for i in 0..4 {
        assert_eq!(results.matches(EntityId::from_bits(i), 0), i < 2);
        assert_eq!(results.matches(EntityId::from_bits(i), 1), i >= 2);
    }
}
