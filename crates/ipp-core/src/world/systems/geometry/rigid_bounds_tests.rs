use super::*;
use crate::{
    Batch, Command, ComponentValue, EntityPlacementRef, EntityRef, HostRuntime,
    components::Transform,
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
    let mut host = HostRuntime::default();
    let world_id = host
        .create_world(
            Default::default(),
            &[
                crate::systems::animation::AnimationSystem::ID,
                crate::systems::asset_dependencies::AssetDependencySystem::ID,
                crate::systems::hierarchy::HierarchySystem::ID,
                crate::systems::look_at::LookAtSystem::ID,
                crate::systems::hierarchy::FinalPropagationSystem::ID,
                crate::systems::geometry::GeometrySystem::ID,
            ],
        )
        .unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 0,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::PlaceEntity {
                    entity: EntityRef::Alias(1),
                    placement: EntityPlacementRef {
                        parent: Some(EntityRef::Alias(0)),
                        before: None,
                    },
                },
            ],
        })
        .unwrap();
    let entities = world.step(0.0).unwrap().outcomes.remove(0).result.unwrap();
    let parent = entities[0].1;
    let entity = entities[1].1;
    // SAFETY: The test retains this entity generation until the binding is
    // dropped. All reads end before the next exclusive World update.
    let model = unsafe { ObjectTransformBinding::bind(world.world, entity) };
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
        let time = frame as f32 * 0.031;
        world
            .enqueue(Batch {
                id: frame + 2,
                operations: vec![
                    Command::insert_value(
                        EntityRef::Handle(parent),
                        ComponentValue::Transform(Transform {
                            x: time,
                            y: -2.0 * time,
                            z: 0.4,
                            sx: 1.0 + time,
                            sy: 0.7 + time,
                            sz: 1.3,
                            ..Default::default()
                        }),
                    ),
                    Command::insert_value(
                        EntityRef::Handle(entity),
                        ComponentValue::Transform(Transform {
                            qz: (time / 2.0).sin(),
                            qw: (time / 2.0).cos(),
                            ..Default::default()
                        }),
                    ),
                ],
            })
            .unwrap();
        world.step(0.0).unwrap().outcomes.remove(0).result.unwrap();
        let model = world
            .world
            .state
            .links
            .transform(entity)
            .unwrap()
            .value()
            .unwrap();
        let matrix = model.matrix();
        assert!(state.update_rigid(world.world));
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
        assert!(state.update_rigid(world.world));
        assert!(!state.changed);
    }

    world
        .world
        .state
        .links
        .transform_mut(entity)
        .unwrap()
        .clear();
    assert!(
        !state.update_rigid(world.world),
        "invalid poses use error evaluation"
    );
    state.program = None;
    assert!(
        !state.update_rigid(world.world),
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

#[test]
fn reflected_sheared_enclosures_match_independent_transformed_corners() {
    for frame in 0..128 {
        let time = f64::from(frame) * 0.031;
        let matrix = [
            -1.0 - time,
            0.0,
            0.0,
            0.0,
            time.sin(),
            0.7 + time,
            0.0,
            0.0,
            0.1,
            time.cos(),
            1.3,
            0.0,
            time,
            -2.0 * time,
            0.4,
            1.0,
        ];
        let min = [-1.7, 0.25, -3.0];
        let max = [2.1, 1.3, -0.1];
        let enclosure =
            super::super::GeometryEnclosure::transformed_box(min, max, &matrix).unwrap();
        let expected = corner_bounds(min, max, &matrix);
        for (actual, expected) in enclosure
            .bounds
            .iter()
            .flatten()
            .zip(expected.iter().flatten())
        {
            assert!((actual - expected).abs() < 1e-12);
        }
    }
}
