use super::*;
use ipp_core::{
    Batch, CameraStateChange, CameraStatePatch, Command, ComponentValue, EntityId, EntityRef,
    components::{Camera, Transform},
};

use ipp_core::ErrorReason;

struct CameraPlatform {
    viewport: Option<(u32, u32)>,
}

impl HostServices for CameraPlatform {
    const NAME: &'static str = "camera-test";

    fn initialize(_world: &mut ipp_core::HostRuntime) -> Result<Self, String> {
        Ok(Self {
            viewport: None,
        })
    }

    fn render_viewport(&self) -> Option<(u32, u32)> {
        self.viewport
    }

    fn service_resources(&mut self, _world: &mut ipp_core::HostRuntime) -> Result<(), String> {
        Ok(())
    }
}

fn ready() -> Host<CameraPlatform> {
    let mut session = Host::new().unwrap();
    session.open_session(7).unwrap();
    session
        .test_session()
        .receive(&ipp_protocol::bootstrap())
        .unwrap();
    session.test_session().take_response().unwrap();
    session
}

fn request(request_id: u64, tag: u8) -> Vec<u8> {
    let mut bytes = 7u64.to_le_bytes().to_vec();
    bytes.extend_from_slice(&request_id.to_le_bytes());
    bytes.push(tag);
    bytes
}

fn activate(entity: EntityId) -> Vec<u8> {
    let mut bytes = request(0, 8);
    bytes.extend_from_slice(&entity.to_bits().to_le_bytes());
    bytes
}

fn delete(request_id: u64, entity: EntityId) -> Vec<u8> {
    let mut bytes = request(request_id, 1);
    bytes.extend_from_slice(&request_id.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[2, 0]);
    bytes.extend_from_slice(&entity.to_bits().to_le_bytes());
    bytes
}

fn cameras(session: &mut Host<CameraPlatform>) -> [EntityId; 2] {
    let operations = [1, 2]
        .into_iter()
        .flat_map(|alias| {
            [
                Command::Create {
                    alias,
                    metadata: Default::default(),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(alias),
                    value: ComponentValue::Transform(Transform {
                        z: alias as f32 + 4.0,
                        ..Default::default()
                    }),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(alias),
                    value: ComponentValue::Camera(Camera::default()),
                },
            ]
        })
        .collect();
    session
        .test_session()
        .world_mut()
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    let report = session.test_session().world_mut().step(0.0).unwrap();
    let ids = report.outcomes[0].result.as_ref().unwrap();
    [ids[0].1, ids[1].1]
}

fn camera_change(camera: EntityId, tick: u64) -> Vec<u8> {
    ipp_protocol::encode_response(&Response {
        session: 7,
        request_id: 0,
        tick,
        body: ResponseBody::CameraStateChangedEvent(CameraStateChange {
            tick,
            changes: CameraStatePatch {
                active_camera: Some(camera),
            },
        }),
    })
    .unwrap()
}

#[test]
fn activation_interleaves_with_partial_batches_and_reports_each_selection() {
    let mut session = ready();
    let [first, second] = cameras(&mut session);
    session.test_session().receive(&activate(first)).unwrap();
    session.test_session().receive(&delete(12, first)).unwrap();
    session.test_session().receive(&activate(second)).unwrap();
    session.test_session().receive(&delete(14, first)).unwrap();
    assert_eq!(session.test_session().world().active_camera(), None);
    assert_eq!(session.test_session().world().tick(), 1);
    assert!(session.test_session().take_response().is_none());

    session.tick(0.0).unwrap();
    assert_eq!(session.test_session().world().active_camera(), Some(second));
    assert_eq!(session.test_session().world().entities().len(), 1);
    assert_eq!(
        session.test_session().take_response().unwrap(),
        camera_change(first, 2)
    );
    assert_eq!(
        session.test_session().take_response().unwrap(),
        camera_change(second, 2)
    );
    let rejected = session.test_session().take_response().unwrap();
    assert_eq!(&rejected[8..16], &12u64.to_le_bytes());
    assert_eq!(rejected[24], 1);
    assert_eq!(rejected[41], 1);
    assert!(String::from_utf8_lossy(&rejected).contains("ActiveCamera"));
    let deleted = session.test_session().take_response().unwrap();
    assert_eq!(&deleted[8..16], &14u64.to_le_bytes());
    assert_eq!(deleted[41], 1);
    assert!(String::from_utf8_lossy(&deleted).contains("InvalidEntity"));
    assert_eq!(session.test_session().take_response().unwrap()[24], 4);
    assert!(session.test_session().take_response().is_none());
}

#[test]
fn invalid_noop_and_capacity_rejected_commands_have_no_reply() {
    let mut session = ready();
    session.test_limits(ipp_core::WorldLimits {
        max_queued_batches: 1,
        ..Default::default()
    });
    session
        .test_session()
        .receive(&activate(EntityId::from_bits(0)))
        .unwrap();
    session
        .test_session()
        .receive(&activate(EntityId::from_bits(0)))
        .unwrap();
    session.tick(0.0).unwrap();
    assert_eq!(session.test_session().world().active_camera(), None);
    assert_eq!(session.test_session().take_response().unwrap()[24], 4);
    assert!(session.test_session().take_response().is_none());

    let mut session = ready();
    let [first, _] = cameras(&mut session);
    session.test_session().receive(&activate(first)).unwrap();
    session.test_session().receive(&activate(first)).unwrap();
    session.tick(0.0).unwrap();
    assert_eq!(
        session.test_session().take_response().unwrap(),
        camera_change(first, 2)
    );
    assert_eq!(session.test_session().take_response().unwrap()[24], 4);
    assert!(session.test_session().take_response().is_none());
}

#[test]
fn command_ingress_backpressure_rejects_explicitly_and_draining_recovers() {
    let mut session = ready();
    let command = activate(EntityId::from_bits(0));
    for _ in 0..MAX_PENDING {
        session.test_session().receive(&command).unwrap();
    }
    assert!(session.test_session().receive(&command).is_err());
    assert_eq!(session.test_session().world().tick(), 0);
    session.tick(0.0).unwrap();
    assert_eq!(session.test_session().take_response().unwrap()[24], 4);
    assert!(session.test_session().take_response().is_none());
    session.test_session().receive(&command).unwrap();
    session.tick(0.0).unwrap();
    assert_eq!(session.test_session().world().tick(), 2);
}

#[test]
fn navigation_keeps_command_order_and_emits_no_component_notification() {
    let mut session = ready();
    let [first, _] = cameras(&mut session);
    session.test_session().receive(&activate(first)).unwrap();
    let mut navigate = request(0, 11);
    navigate.push(2);
    navigate.extend_from_slice(&0.5f32.to_le_bytes());
    session.test_session().receive(&navigate).unwrap();
    session.tick(0.0).unwrap();
    assert_eq!(
        session.test_session().take_response().unwrap(),
        camera_change(first, 2)
    );
    assert_eq!(session.test_session().take_response().unwrap()[24], 4);
    assert!(session.test_session().take_response().is_none());
    let entities = session.test_session().world().entities();
    let camera = entities.iter().find(|entity| entity.id == first).unwrap();
    let transform = camera
        .base
        .iter()
        .find_map(|component| match component {
            ComponentValue::Transform(transform) => Some(transform),
            _ => None,
        })
        .unwrap();
    assert!(transform.z > 5.0);
}

fn pick(request_id: u64, width: u32, height: u32) -> Vec<u8> {
    let mut bytes = request(request_id, 9);
    bytes.extend_from_slice(&0.5f32.to_le_bytes());
    bytes.extend_from_slice(&0.5f32.to_le_bytes());
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.push(0);
    bytes
}

fn pick_reply(
    request_id: u64,
    camera: Option<EntityId>,
    tick: u64,
    result: Result<Option<ipp_core::GeometryPickHit>, ErrorReason>,
) -> Vec<u8> {
    ipp_protocol::encode_response(&Response {
        session: 7,
        request_id,
        tick,
        body: ResponseBody::GeometryPickResultEvent(ipp_core::GeometryPickOutcome {
            request_id,
            tick,
            camera,
            result,
        }),
    })
    .unwrap()
}

#[test]
fn queries_observe_final_camera_and_host_surface_without_resizing_or_advancing_time() {
    let mut session = ready();
    let [first, second] = cameras(&mut session);
    session.test_session().receive(&pick(31, 640, 480)).unwrap();
    session.test_session().receive(&activate(first)).unwrap();
    session.test_session().receive(&activate(second)).unwrap();
    session.test_session().receive(&pick(34, 1, 1)).unwrap();
    session.services_mut().viewport = Some((640, 480));
    session.tick(0.0).unwrap();
    assert_eq!(
        session.test_session().take_response().unwrap(),
        camera_change(first, 2)
    );
    assert_eq!(
        session.test_session().take_response().unwrap(),
        camera_change(second, 2)
    );
    assert_eq!(
        session.test_session().take_response().unwrap(),
        pick_reply(31, Some(second), 2, Ok(None))
    );
    assert_eq!(
        session.test_session().take_response().unwrap(),
        pick_reply(34, Some(second), 2, Err(ErrorReason::InvalidViewport))
    );
    assert_eq!(session.test_session().world().time(), 0.0);
    assert_eq!(session.services_mut().viewport, Some((640, 480)));
    session.test_session().take_response().unwrap();

    session.services_mut().viewport = None;
    session.test_session().receive(&pick(35, 1, 1)).unwrap();
    session.tick(0.0).unwrap();
    assert_eq!(
        session.test_session().take_response().unwrap(),
        pick_reply(35, Some(second), 3, Ok(None))
    );
}

#[test]
fn queries_retain_capacity_failure_correlation_and_reject_replaced_sessions() {
    let mut session = ready();
    session.test_limits(ipp_core::WorldLimits {
        max_queued_batches: 1,
        ..Default::default()
    });
    session
        .test_session()
        .receive(&activate(EntityId::from_bits(0)))
        .unwrap();
    session.test_session().receive(&pick(61, 640, 480)).unwrap();
    session.tick(0.0).unwrap();
    assert_eq!(
        session.test_session().take_response().unwrap(),
        pick_reply(61, None, 1, Err(ErrorReason::Capacity))
    );
    assert_eq!(session.test_session().take_response().unwrap()[24], 4);
    assert!(session.test_session().take_response().is_none());

    let mut replacement = Host::<CameraPlatform>::new().unwrap();
    replacement.open_session(8).unwrap();
    replacement
        .test_session()
        .receive(&ipp_protocol::bootstrap())
        .unwrap();
    replacement.test_session().take_response().unwrap();
    assert!(
        replacement
            .test_session()
            .receive(&pick(62, 640, 480))
            .is_err()
    );
    assert_eq!(replacement.test_session().world().tick(), 0);
    assert!(replacement.test_session().session.pending.is_empty());
}
