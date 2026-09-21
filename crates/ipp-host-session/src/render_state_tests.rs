use super::*;
use ipp_core::{RenderState, RenderStateChange, RenderStatePatch, WorldLimits};

struct HeadlessPlatform;

impl HostServices for HeadlessPlatform {
    const NAME: &'static str = "render-state-test";

    fn initialize(_world: &mut ipp_core::HostRuntime) -> Result<Self, String> {
        Ok(Self)
    }

    fn service_resources(&mut self, _world: &mut ipp_core::HostRuntime) -> Result<(), String> {
        Ok(())
    }
}

fn ready(id: u64) -> Host<HeadlessPlatform> {
    let mut session = Host::new().unwrap();
    session.open_session(id).unwrap();
    session
        .test_session()
        .receive(&ipp_protocol::bootstrap())
        .unwrap();
    session.test_session().take_response().unwrap();
    session
}

fn patch(changes: RenderStatePatch) -> Vec<u8> {
    let mut bytes = 7u64.to_le_bytes().to_vec();
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.push(10);
    let mask = u16::from(changes.show_all_debug_geometries.is_some())
        | (u16::from(changes.debug_geometry_color.is_some()) << 1)
        | (u16::from(changes.ambient_light.is_some()) << 2);
    bytes.extend_from_slice(&mask.to_le_bytes());
    if let Some(show) = changes.show_all_debug_geometries {
        bytes.push(u8::from(show));
    }
    if let Some(color) = changes.debug_geometry_color {
        for channel in color {
            bytes.extend_from_slice(&channel.to_le_bytes());
        }
    }
    if let Some(color) = changes.ambient_light {
        for channel in color {
            bytes.extend_from_slice(&channel.to_le_bytes());
        }
    }
    bytes
}

fn change(changes: RenderStatePatch) -> Vec<u8> {
    ipp_protocol::encode_response(&Response {
        session: 7,
        request_id: 0,
        tick: 1,
        body: ResponseBody::RenderStateUpdatedEvent(RenderStateChange {
            tick: 1,
            changes,
        }),
    })
    .unwrap()
}

#[test]
fn sparse_updates_queue_in_order_and_invalid_patches_preserve_all_committed_settings() {
    let mut session = ready(7);
    let first = RenderStatePatch {
        show_all_debug_geometries: Some(true),
        debug_geometry_color: None,
        ambient_light: Some([2.0, 0.5, 0.0]),
    };
    let color = RenderStatePatch {
        show_all_debug_geometries: None,
        debug_geometry_color: Some([0.25, 0.5, 1.0]),
        ambient_light: None,
    };
    let invalid = RenderStatePatch {
        show_all_debug_geometries: Some(false),
        debug_geometry_color: Some([0.0, 2.0, 0.0]),
        ambient_light: None,
    };
    for changes in [first, color, invalid, RenderStatePatch::default(), color] {
        session.test_session().receive(&patch(changes)).unwrap();
    }
    assert_eq!(
        session.test_session().world().render_state(),
        RenderState::default()
    );
    assert_eq!(session.test_session().world().tick(), 0);
    assert!(session.test_session().take_response().is_none());

    session.tick(0.0).unwrap();
    assert_eq!(
        session.test_session().take_response().unwrap(),
        change(first)
    );
    assert_eq!(
        session.test_session().take_response().unwrap(),
        change(color)
    );
    assert_eq!(
        session.test_session().world().render_state(),
        RenderState {
            show_all_debug_geometries: true,
            debug_geometry_color: [0.25, 0.5, 1.0],
            ambient_light: [2.0, 0.5, 0.0],
        }
    );
    assert_eq!(session.test_session().world().time(), 0.0);
    assert_eq!(session.test_session().take_response().unwrap()[24], 4);
    assert!(session.test_session().take_response().is_none());
}

#[test]
fn command_capacity_and_replacement_sessions_preserve_state_without_replies() {
    let mut session = ready(7);
    session.test_limits(WorldLimits {
        max_queued_batches: 1,
        ..Default::default()
    });
    let changes = RenderStatePatch {
        show_all_debug_geometries: Some(true),
        ambient_light: Some([0.25, 0.5, 1.0]),
        ..Default::default()
    };
    session.test_session().receive(&patch(changes)).unwrap();
    session.test_session().receive(&patch(changes)).unwrap();
    session.tick(0.0).unwrap();
    assert_eq!(
        session.test_session().take_response().unwrap(),
        change(changes)
    );
    assert_eq!(session.test_session().take_response().unwrap()[24], 4);
    assert!(session.test_session().take_response().is_none());

    let mut replacement = ready(8);
    assert!(replacement.test_session().receive(&patch(changes)).is_err());
    assert_eq!(
        replacement.test_session().world().render_state(),
        RenderState::default()
    );
    assert_eq!(replacement.test_session().world().tick(), 0);
}

#[test]
fn malformed_patch_masks_and_booleans_never_enter_the_world() {
    let mut session = ready(7);
    let changes = RenderStatePatch {
        show_all_debug_geometries: Some(true),
        ..Default::default()
    };
    let mut boolean = patch(changes);
    boolean[19] = 2;
    assert!(session.test_session().receive(&boolean).is_err());
    let mut mask = patch(RenderStatePatch::default());
    mask[17] = 8;
    assert!(session.test_session().receive(&mask).is_err());
    assert_eq!(
        session.test_session().world().render_state(),
        RenderState::default()
    );
    assert_eq!(session.test_session().world().tick(), 0);
    assert!(session.test_session().session.pending.is_empty());
}

#[test]
fn runtime_originated_updates_keep_zero_identity_and_order_before_client_patches() {
    let mut session = ready(7);
    let runtime = RenderStatePatch {
        show_all_debug_geometries: Some(true),
        ..Default::default()
    };
    let client = RenderStatePatch {
        show_all_debug_geometries: Some(false),
        ..Default::default()
    };
    session
        .test_session()
        .world_mut()
        .enqueue_render_state_update(runtime)
        .unwrap();
    session.test_session().receive(&patch(client)).unwrap();
    session.tick(0.0).unwrap();
    assert_eq!(
        session.test_session().take_response().unwrap(),
        change(runtime)
    );
    assert_eq!(
        session.test_session().take_response().unwrap(),
        change(client)
    );
    assert!(
        !session
            .test_session()
            .world()
            .render_state()
            .show_all_debug_geometries
    );
}
