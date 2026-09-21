use super::*;
use crate::{
    ComponentValue,
    components::Hierarchy,
    components::registry::ComponentStorage,
    systems::{geometry::program::GeometryProgram, hierarchy::ObjectTransformBinding},
};

fn corner_bounds(min: [f64; 3], max: [f64; 3], matrix: &[f64; 16]) -> [[f64; 3]; 2] {
    let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for corner in 0..8 {
        let local: [f64; 3] = std::array::from_fn(|i| {
            if corner & (1 << i) == 0 {
                min[i]
            } else {
                max[i]
            }
        });
        for axis in 0..3 {
            let point = matrix[axis] * local[0]
                + matrix[4 + axis] * local[1]
                + matrix[8 + axis] * local[2]
                + matrix[12 + axis];
            bounds[0][axis] = bounds[0][axis].min(point);
            bounds[1][axis] = bounds[1][axis].max(point);
        }
    }
    bounds
}

#[test]
fn rigid_motion_updates_bounds_and_exact_query_shape_without_replacing_storage() {
    let min = [-1.7, 0.25, -3.0];
    let max = [2.1, 1.3, -0.1];
    let shape = GeometryShape::Box {
        min,
        max,
    };
    let entity = EntityId::from_bits(1 << 32);
    let mut storage = ComponentStorage::default();
    storage.reserve(1);
    storage.set(0, ComponentValue::Hierarchy(Hierarchy::default()));
    // SAFETY: The test retains this occupied hierarchy slot until the state and
    // binding are dropped. Each read ends before the next exclusive pose write.
    let model = unsafe { ObjectTransformBinding::bind(&storage, entity) };
    let mut state = GeometryEvaluationState {
        program: Some(GeometryProgram::Rigid {
            shape,
            model,
        }),
        evaluated: Ok(CompoundGeometryShape {
            parts: vec![TransformedGeometryShape {
                shape,
                transform: GeometryShapeTransform::default(),
            }],
        }),
        ..Default::default()
    };
    let address = state.evaluated().unwrap().parts.as_ptr();
    let capacity = state.evaluated().unwrap().parts.capacity();
    for frame in 0..128 {
        let t = f64::from(frame) * 0.031;
        // Reflected, nonuniformly scaled and sheared placements exercise the
        // exact affine query shape as well as its conservative world enclosure.
        let matrix = [
            -1.0 - t,
            0.0,
            0.0,
            0.0,
            t.sin(),
            0.7 + t,
            0.0,
            0.0,
            0.1,
            t.cos(),
            1.3,
            0.0,
            t,
            -2.0 * t,
            0.4,
            1.0,
        ];
        let model = GeometryShapeTransform::new(matrix).unwrap();
        storage.hierarchy_mut(0).unwrap().runtime.world = Some(model);
        assert!(state.update_rigid(&storage));
        assert!(state.changed);
        assert!(state.culling_valid);
        let enclosure = state.enclosure.unwrap();
        let expected = corner_bounds(min, max, &matrix);
        for (actual, expected) in enclosure
            .bounds
            .iter()
            .flatten()
            .zip(expected.iter().flatten())
        {
            assert!((actual - expected).abs() < 1e-12);
        }
        let half: [f64; 3] = std::array::from_fn(|i| (expected[1][i] - expected[0][i]) / 2.0);
        let radius = half.iter().map(|v| v * v).sum::<f64>().sqrt();
        assert!((enclosure.radius - radius).abs() < 1e-12);
        assert_eq!(state.visual, state.enclosure);
        let evaluated = state.evaluated().unwrap();
        assert_eq!(evaluated.parts.as_ptr(), address);
        assert_eq!(evaluated.parts.capacity(), capacity);
        assert_eq!(evaluated.parts[0].shape, shape);
        assert_eq!(evaluated.parts[0].transform, model);
        assert_eq!(evaluated.bounds(), Some(enclosure.bounds));
        assert!(state.update_rigid(&storage));
        assert!(!state.changed);
    }

    storage.hierarchy_mut(0).unwrap().runtime.world = None;
    assert!(
        !state.update_rigid(&storage),
        "invalid poses use error evaluation"
    );
    storage.hierarchy_mut(0).unwrap().runtime.world = Some(GeometryShapeTransform::default());
    state.program = None;
    assert!(
        !state.update_rigid(&storage),
        "invalidated bindings require preparation"
    );
}

#[test]
fn direct_box_enclosure_preserves_flat_axes_and_rejects_overflow() {
    let matrix = GeometryShapeTransform::default().matrix();
    let result = super::super::GeometryEnclosure::transformed_box(
        [-2.0, 1.0, 3.0],
        [2.0, 1.0, 3.0],
        &matrix,
    )
    .unwrap();
    assert_eq!(result.center, [0.0, 1.0, 3.0]);
    assert_eq!(result.radius, 2.0);
    let mut overflow = matrix;
    overflow[0] = f64::MAX;
    assert!(
        super::super::GeometryEnclosure::transformed_box([-2.0; 3], [2.0; 3], &overflow,).is_none()
    );
}
