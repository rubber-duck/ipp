//! Host services remain usable independently of selected World dependency tracking.

mod support;
use support::task_scheduler::HostTaskTestDriver;
use support::task_scheduler::WorldTaskTestDriver;

use ipp_core::{
    ErrorReason, HostRuntime, WorldId, WorldLimits, services::asset_management::*,
    systems::animation::*,
};

fn clip_bytes() -> Vec<u8> {
    AnimationClip::new(
        1.0,
        vec![AnimationTrack {
            target: AnimationTrackTarget::EntityLink,
            keys: vec![AnimationKeyframe {
                time: 0.0,
                value: AnimationValue::EntityPlacement(AnimationEntityPlacementKey {
                    parent: None,
                    before: None,
                }),
                interpolation: AnimationInterpolation::Step,
            }],
        }],
    )
    .unwrap()
    .encode()
}

fn selected_world(host: &mut HostRuntime, animation: bool) -> WorldId {
    let selected = if animation {
        vec![AnimationSystem::ID]
    } else {
        Vec::new()
    };
    host.create_world(WorldLimits::default(), &selected)
        .unwrap()
}

#[test]
fn selected_world_observes_real_host_resources_without_upload_queue() {
    for animation in [false, true] {
        let mut host = crate::support::task_scheduler::host();
        let world_id = selected_world(&mut host, animation);
        let peer = selected_world(&mut host, animation);
        let source = AssetSource {
            kind: ANIMATION_TYPE,
            uri: std::sync::Arc::<str>::from(format!("producer://{}/10/9", world_id.0)),
            variant: 0,
        };
        host.asset_resources_mut()
            .register_client_source(world_id, source.clone(), clip_bytes())
            .unwrap();
        host.progress_assets_for_test();
        let key = host.asset_resources().find(&source).unwrap();
        assert!(
            host.asset_resources()
                .get_typed::<AnimationClip>(key)
                .is_some()
        );

        let mut world = host.world_mut(world_id).unwrap();
        assert_eq!(world.system_ids().count(), usize::from(animation));
        let snapshots = world.resource_snapshots();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].id, key.to_u64());
        assert_eq!(snapshots[0].status, AssetLoadStatus::Loaded);
        assert_eq!(world.resource_page(0, key.to_u64(), 1), snapshots);
        assert!(!world.take_asset_events().unwrap().is_empty());
        assert!(world.take_asset_outcomes().is_empty());
        assert_eq!(world.pending_asset_uploads(), 0);
        assert_eq!(
            world.enqueue_asset(AssetUpload {
                id: 7,
                key: AssetUploadIdentity {
                    kind: ANIMATION_TYPE,
                    asset: 11,
                    variant: 0
                },
                bytes: clip_bytes(),
            }),
            Err(ErrorReason::UnsupportedDependency)
        );
        assert_eq!(world.pending_asset_uploads(), 0);
        assert!(
            world
                .resolve_asset_key(AssetUploadIdentity {
                    kind: ANIMATION_TYPE,
                    asset: 11,
                    variant: 0
                })
                .is_none()
        );
        drop(world);

        assert!(
            host.world_mut(peer)
                .unwrap()
                .resource_snapshots()
                .is_empty()
        );
        host.asset_resources_mut()
            .retain_prepared_source(peer, &source)
            .unwrap();
        host.world_mut(world_id)
            .unwrap()
            .release_asset_upload(AssetUploadIdentity {
                kind: ANIMATION_TYPE,
                asset: 9,
                variant: 0,
            });
        host.flush_resource_lifecycle();
        assert!(
            host.world_mut(world_id)
                .unwrap()
                .resource_snapshots()
                .is_empty()
        );
        assert_eq!(host.world_mut(peer).unwrap().resource_snapshots().len(), 1);
        assert!(
            host.asset_resources()
                .get_typed::<AnimationClip>(key)
                .is_some()
        );
    }
}

#[test]
fn selected_world_stream_helpers_progress_and_complete_shared_loaders() {
    for animation in [false, true] {
        for chunked in [false, true] {
            let mut host = crate::support::task_scheduler::host();
            let id = selected_world(&mut host, animation);
            let mut world = host.world_mut(id).unwrap();
            world.register_stream_resource_provider("fixture").unwrap();
            let key = world
                .asset_resources_mut()
                .get_or_create(AssetSource {
                    kind: ANIMATION_TYPE,
                    uri: "fixture:clip".into(),
                    variant: 0,
                })
                .unwrap();
            world.set_renderer_asset_loading(true);
            world.poll_assets_for_test();
            let requests = world.take_resource_requests();
            assert_eq!(requests.len(), 1);
            assert_eq!(
                requests[0].source,
                std::sync::Arc::<str>::from("fixture:clip")
            );
            let bytes = clip_bytes();
            if chunked {
                assert!(world.asset_input_chunk(requests[0].id, &bytes).unwrap());
                assert!(world.asset_input_bytes() >= bytes.len());
                world.asset_input_end(requests[0].id, Ok(()));
            } else {
                world.complete_resource(requests[0].id, Ok(bytes)).unwrap();
            }
            world.poll_all_assets();
            support::task_scheduler::poll_ready();
            world.poll_all_assets();
            assert!(
                world
                    .asset_resources()
                    .get_typed::<AnimationClip>(key)
                    .is_some()
            );
            assert!(world.take_resource_cancellations().is_empty());
            assert!(world.take_resource_requests().is_empty());
            assert!(world.take_asset_outcomes().is_empty());
            world.set_renderer_asset_loading(false);
            world.poll_assets_for_test();
            drop(world);
            host.flush_resource_lifecycle();
        }
    }
}
