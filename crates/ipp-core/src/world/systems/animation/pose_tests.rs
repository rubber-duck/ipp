use super::{Transform, sample_joints};
use crate::systems::animation::*;
use crate::{ComponentValue, EntityId, services::asset_management::AssetKey};

#[test]
fn borrowed_pose_sampling_matches_owned_sampling_for_all_interpolations_and_layers() {
    use super::super::driver::make_driver;
    let joint = |x: f32| Transform {
        x,
        qz: (x * 0.1).sin(),
        qw: (x * 0.1).cos(),
        ..Default::default()
    };
    for interpolation in [
        AnimationInterpolation::Step,
        AnimationInterpolation::Linear,
        AnimationInterpolation::Bezier {
            time1: 0.2,
            value1: vec![joint(3.0), joint(-2.0)],
            time2: 1.4,
            value2: vec![joint(-1.0), joint(5.0)],
        },
    ] {
        let target = AnimationTrackTarget::Joints(vec![0, 2]);
        let clip = AnimationClip::new(
            2.0,
            vec![AnimationTrack::<Vec<Transform>> {
                target: target.clone(),
                keys: vec![
                    AnimationKeyframe {
                        time: 0.0,
                        value: vec![joint(0.0), joint(1.0)],
                        interpolation,
                    },
                    AnimationKeyframe {
                        time: 2.0,
                        value: vec![joint(2.0), joint(4.0)],
                        interpolation: AnimationInterpolation::Step,
                    },
                ],
            }],
        )
        .unwrap();
        for additive in [false, true] {
            for weight in [0.25, 0.75, 1.0] {
                let entity = EntityId::from_bits(1 << 32);
                let source = AssetKey::from_u64(1);
                let original = vec![joint(6.0), joint(8.0)];
                let driver = make_driver(
                    AnimationDriverDescription {
                        target: entity,
                        property: target.clone(),
                        source: "asset://10/1".into(),
                        variant: 0,
                        track: 0,
                        weight,
                        additive,
                        reference_time: 0.4,
                        repeat: true,
                    },
                    1,
                    target.clone(),
                    AssetKey::from_u64(2),
                    clip.duration(),
                    AnimationValue::Pose(original.clone()),
                    Some(source),
                )
                .unwrap();
                let mut storage = crate::components::registry::ComponentStorage::default();
                storage.reserve(1);
                storage.set(
                    0,
                    ComponentValue::Skeleton(crate::components::Skeleton {
                        runtime: crate::systems::skeleton::SkeletonRuntimeState {
                            pose: Some(crate::systems::skeleton::SkeletonPoseState {
                                source,
                                valid: true,
                                local: vec![joint(6.0), joint(7.0), joint(8.0)].into_boxed_slice(),
                                global: vec![[0.0; 16]; 3].into_boxed_slice(),
                                evaluation: vec![Transform::default(); 3].into_boxed_slice(),
                                sampled: vec![false; 3].into_boxed_slice(),
                            }),
                        },
                        ..Default::default()
                    }),
                );
                let address = storage
                    .skeleton(0)
                    .unwrap()
                    .runtime
                    .pose
                    .as_ref()
                    .unwrap()
                    .local
                    .as_ptr();
                for time in [-0.1, 0.0, 0.001, 0.2, 0.999, 1.5, 1.999, 2.0, 2.2] {
                    driver
                        .runtime_target()
                        .write_joint_slice(&mut storage, entity, &original)
                        .unwrap();
                    let expected = driver
                        .sample(&clip, time, AnimationValue::Pose(original.clone()))
                        .unwrap();
                    sample_joints(
                        driver.as_ref(),
                        clip.typed_track::<Vec<Transform>>(0).unwrap(),
                        clip.duration(),
                        time,
                        &mut storage,
                    )
                    .unwrap();
                    assert_eq!(
                        driver
                            .runtime_target()
                            .read_joints(&storage, entity)
                            .unwrap(),
                        expected,
                        "{additive}/{weight}/{time}"
                    );
                    let pose = storage.skeleton(0).unwrap().runtime.pose.as_ref().unwrap();
                    assert_eq!(pose.local.as_ptr(), address);
                    assert_eq!(pose.local[1], joint(7.0));
                    assert_eq!(&*pose.sampled, &[true, false, true]);
                }
            }
        }
    }
}
