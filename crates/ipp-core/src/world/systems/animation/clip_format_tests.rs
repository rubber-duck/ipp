use super::*;
use crate::services::asset_management::writer::AssetEncoder;

fn track(target: AnimationTrackTarget, value: AnimationValue) -> AnimationTrack {
    AnimationTrack {
        target,
        keys: vec![AnimationKeyframe {
            time: 0.0,
            value,
            interpolation: AnimationInterpolation::Step,
        }],
    }
}

fn assert_exact_v4_size(tracks: Vec<AnimationTrack>) {
    let clip = AnimationClip::new(2.0, tracks).unwrap();
    let encoded = clip.encode();

    assert_eq!(encoded.len(), clip.bytes);
    assert!(clip.encode_asset(encoded.len() - 1).is_err());
    assert_eq!(clip.encode_asset(encoded.len()).unwrap(), encoded);
    assert_eq!(AnimationClip::decode(&encoded).unwrap().encode(), encoded);
}

#[test]
fn v4_encoded_size_matches_each_target_and_mixed_tracks() {
    let static_track = track(
        AnimationTrackTarget::AnimationProperty(AnimationProperty {
            component: ComponentValue::SCALAR,
            offsets: vec![0],
        }),
        AnimationValue::Field(FieldValue::F32(1.0)),
    );
    let dynamic_track = track(
        AnimationTrackTarget::DynamicProperty {
            component: ComponentValue::CUSTOM_MATERIAL,
            name: "tint".into(),
        },
        AnimationValue::Field(FieldValue::Dynamic(crate::DynamicValue::Vec4([1.0; 4]))),
    );
    let structural_track = track(
        AnimationTrackTarget::EntityLink,
        AnimationValue::EntityPlacement(AnimationEntityPlacementKey {
            parent: Some(0),
            before: None,
        }),
    );

    for track in [&static_track, &dynamic_track, &structural_track] {
        assert_exact_v4_size(vec![track.clone()]);
    }
    let mixed = vec![static_track, dynamic_track, structural_track];

    let mixed = {
        let pose_track = track(
            AnimationTrackTarget::Joints(vec![0]),
            AnimationValue::Pose(vec![crate::components::Transform::default()]),
        );
        assert_exact_v4_size(vec![pose_track.clone()]);
        let mut mixed = mixed;
        mixed.push(pose_track);
        mixed
    };

    assert_exact_v4_size(mixed);
}
