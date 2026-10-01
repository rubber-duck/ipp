use super::*;
use ipp_core::{
    Batch, Command, ComponentValue, EntityRef, ErrorReason, OutputKind, OutputRef, WorldViewport,
    components::{Camera, Transform},
};

struct CameraPlatform;

struct FrameCheckFailure(std::sync::Arc<std::sync::atomic::AtomicU64>);

impl ipp_core::systems::SystemFactory for FrameCheckFailure {
    fn id(&self) -> ipp_core::systems::SystemId {
        ipp_core::systems::SystemId("test.frame-check-failure")
    }

    fn create(
        &self,
        _: &mut ipp_core::systems::SystemInitContext<'_>,
    ) -> Result<Box<dyn ipp_core::systems::System>, ipp_core::systems::SystemInitError> {
        Ok(Box::new(Self(self.0.clone())))
    }
}

impl ipp_core::systems::System for FrameCheckFailure {
    fn prepare_frame(
        &mut self,
        context: &mut ipp_core::systems::SystemUpdateContext<'_, '_>,
    ) -> Result<(), ErrorReason> {
        if self.0.load(std::sync::atomic::Ordering::Relaxed) == context.world.id().0 {
            Err(ErrorReason::Capacity)
        } else {
            Ok(())
        }
    }

    fn update(&mut self, _: &mut ipp_core::systems::SystemUpdateContext<'_, '_>) {}
}

impl HostServices for CameraPlatform {
    const NAME: &'static str = "camera-test";

    fn initialize(_: &mut ipp_core::HostRuntime) -> Result<Self, String> {
        Ok(Self)
    }

    fn service_resources(&mut self, _: &mut ipp_core::HostRuntime) -> Result<(), String> {
        Ok(())
    }
}

/// The Camera entity and component lifetime a camera output selects.
fn camera_target(output: ipp_core::OutputRef) -> (ipp_core::EntityId, u64) {
    let ipp_core::OutputTarget::Camera {
        entity,
        incarnation,
    } = output.target()
    else {
        panic!("camera output");
    };
    (entity, incarnation)
}

fn ready() -> Host<CameraPlatform> {
    let mut host = Host::new().unwrap();
    host.open_session(7, crate::host::TEST_CAMERA_SYSTEMS)
        .unwrap();
    host.test_session()
        .receive(&ipp_protocol::bootstrap())
        .unwrap();
    host.test_session().take_response().unwrap();
    host
}

fn viewport() -> WorldViewport {
    WorldViewport {
        width: 640,
        height: 480,
        device_pixel_ratio: 1.0,
    }
}

fn navigation(
    request: u64,
    binding: ipp_core::RootOutputBinding,
    source: Option<ipp_core::WorldPublicationId>,
) -> Vec<u8> {
    let mut bytes = 7u64.to_le_bytes().to_vec();
    bytes.extend(request.to_le_bytes());
    bytes.push(40);
    let output = binding.output;
    bytes.extend(output.world().id().0.to_le_bytes());
    bytes.extend(output.world().incarnation().to_le_bytes());
    let (entity, incarnation) = camera_target(output);
    bytes.push(1);
    bytes.extend(entity.to_bits().to_le_bytes());
    bytes.extend(incarnation.to_le_bytes());
    bytes.extend(binding.viewport.width.to_le_bytes());
    bytes.extend(binding.viewport.height.to_le_bytes());
    bytes.extend(binding.viewport.device_pixel_ratio.to_le_bytes());
    let pair = binding.generation.identity();
    bytes.extend(pair.0.to_le_bytes());
    bytes.extend(pair.1.to_le_bytes());
    bytes.push(u8::from(source.is_some()));
    if let Some(source) = source {
        let pair = source.identity();
        bytes.extend(pair.0.to_le_bytes());
        bytes.extend(pair.1.to_le_bytes());
    }
    bytes.extend(2u32.to_le_bytes());
    bytes.extend(std::f32::consts::LN_2.to_le_bytes());
    bytes.extend(0f32.to_le_bytes());
    bytes
}

#[test]
fn camera_navigation_is_correlated_and_equal_rebind_rejects_without_fallback() {
    use ipp_core::systems::camera::CameraPublication;
    let mut host = ready();
    let output = camera(&mut host, 0.0);
    host.runtime_mut()
        .set_root_output(output, viewport())
        .unwrap();
    host.tick(0.0).unwrap();
    responses(&mut host);
    let binding = host
        .runtime()
        .root_output_binding(output.world())
        .unwrap()
        .unwrap();
    let source = host
        .runtime()
        .latest_publication(output.world().id())
        .unwrap();
    let previous = host
        .runtime()
        .output(source, output)
        .unwrap()
        .data::<CameraPublication>()
        .unwrap()
        .projection
        .focus_distance;
    host.test_session()
        .receive(&navigation(71, binding, Some(source)))
        .unwrap();
    host.tick(0.0).unwrap();
    let tick = host.test_session().world().tick();
    let expected = ipp_protocol::encode_response(&Response {
        session: 7,
        request_id: 71,
        tick,
        body: ResponseBody::CameraNavigated,
    })
    .unwrap();
    assert_eq!(&*responses(&mut host)[&71], expected.as_slice());
    let source = host
        .runtime()
        .latest_publication(output.world().id())
        .unwrap();
    assert!(
        (host
            .runtime()
            .output(source, output)
            .unwrap()
            .data::<CameraPublication>()
            .unwrap()
            .projection
            .focus_distance
            - previous * 2.0)
            .abs()
            < 1e-5
    );
    host.test_session()
        .receive(&navigation(72, binding, Some(source)))
        .unwrap();
    host.runtime_mut()
        .set_root_output(output, viewport())
        .unwrap();
    host.tick(0.0).unwrap();
    let reply = responses(&mut host).remove(&72).unwrap();
    assert_eq!(reply[24], 255);
    let source = host
        .runtime()
        .latest_publication(output.world().id())
        .unwrap();
    assert!(
        (host
            .runtime()
            .output(source, output)
            .unwrap()
            .data::<CameraPublication>()
            .unwrap()
            .projection
            .focus_distance
            - previous * 2.0)
            .abs()
            < 1e-5
    );
}

fn camera(host: &mut Host<CameraPlatform>, horizontal: f32) -> OutputRef {
    let world = host.session_world(7).unwrap();
    host.runtime_mut()
        .world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::insert_value(
                    EntityRef::Alias(1),
                    ComponentValue::Camera(Camera::default()),
                ),
                Command::insert_value(
                    EntityRef::Alias(1),
                    ComponentValue::Transform(Transform {
                        x: horizontal,
                        z: 5.0,
                        ..Default::default()
                    }),
                ),
            ],
        })
        .unwrap();

    let entity = host
        .runtime_mut()
        .frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap()[0]
        .1;

    host.runtime()
        .bind_output(
            host.runtime().world_ref(world).unwrap(),
            entity,
            OutputKind::Camera,
        )
        .unwrap()
}

fn pick(request_id: u64, output: OutputRef, viewport: WorldViewport) -> Vec<u8> {
    let mut bytes = 7u64.to_le_bytes().to_vec();
    bytes.extend(request_id.to_le_bytes());
    bytes.extend([9, 0]);
    bytes.extend(output.world().id().0.to_le_bytes());
    bytes.extend(output.world().incarnation().to_le_bytes());
    let (entity, incarnation) = camera_target(output);
    bytes.push(1);
    bytes.extend(entity.to_bits().to_le_bytes());
    bytes.extend(incarnation.to_le_bytes());
    bytes.extend(viewport.width.to_le_bytes());
    bytes.extend(viewport.height.to_le_bytes());
    bytes.extend(viewport.device_pixel_ratio.to_le_bytes());
    bytes.extend(0.5f32.to_le_bytes());
    bytes.extend(0.5f32.to_le_bytes());
    bytes.push(0);
    bytes
}

fn responses(host: &mut Host<CameraPlatform>) -> std::collections::BTreeMap<u64, ReliableResponse> {
    let mut replies = std::collections::BTreeMap::new();
    while let Some(bytes) = host.test_session().take_response() {
        let request_id = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        if request_id != 0 {
            replies.insert(request_id, bytes);
        }
    }
    replies
}

fn expected(
    request_id: u64,
    tick: u64,
    result: Result<(ipp_core::ViewDescriptor, Option<ipp_core::ViewPickHit>), ErrorReason>,
) -> Vec<u8> {
    ipp_protocol::encode_response(&Response {
        session: 7,
        request_id,
        tick,
        body: ResponseBody::GeometryPickResultEvent(ipp_protocol::views::ViewQueryOutcome {
            request_id,
            tick,
            result,
        }),
    })
    .unwrap()
}

#[test]
fn completed_view_replies_use_selected_root_not_legacy_active_camera_and_never_fallback() {
    let mut host = ready();
    let selected = camera(&mut host, 0.0);
    let legacy = camera(&mut host, 100.0);
    let world = selected.world().id();
    host.runtime_mut()
        .world_mut(world)
        .unwrap()
        .enqueue_camera_activate(legacy.camera_entity().unwrap())
        .unwrap();
    host.runtime_mut()
        .set_root_output(selected, viewport())
        .unwrap();
    host.test_session()
        .receive(&pick(31, selected, viewport()))
        .unwrap();
    host.test_session()
        .receive(&pick(
            32,
            selected,
            WorldViewport {
                width: 1,
                height: 1,
                ..viewport()
            },
        ))
        .unwrap();
    host.tick(0.0).unwrap();
    let view = host
        .runtime()
        .resolve_view(ipp_core::ViewQueryTarget::RootView {
            output: selected,
            expected_viewport: viewport(),
        })
        .unwrap();
    let tick = host.test_session().world().tick();
    let replies = responses(&mut host);
    assert_eq!(
        &*replies[&31],
        expected(31, tick, Ok((view, None))).as_slice()
    );
    assert_eq!(
        &*replies[&32],
        expected(32, tick, Err(ErrorReason::InvalidViewport)).as_slice()
    );
    assert_eq!(
        host.test_session().world().active_camera(),
        Some(legacy.camera_entity().unwrap())
    );

    host.runtime_mut().clear_root_output(world);
    host.test_session()
        .receive(&pick(33, selected, viewport()))
        .unwrap();
    host.tick(0.0).unwrap();
    let tick = host.test_session().world().tick();
    assert_eq!(
        &*responses(&mut host)[&33],
        expected(33, tick, Err(ErrorReason::InvalidEntity)).as_slice()
    );
    assert!(host.runtime().root_output(world).is_none());
}

#[test]
fn completed_view_query_is_world_and_session_fenced() {
    let mut host = ready();
    let foreign_world = host
        .runtime_mut()
        .create_world(Default::default(), &[])
        .unwrap();
    let output = camera(&mut host, 0.0);
    let mut bytes = pick(41, output, viewport());
    bytes[18..26].copy_from_slice(&foreign_world.0.to_le_bytes());
    host.test_session().receive(&bytes).unwrap();
    host.tick(0.0).unwrap();
    let tick = host.test_session().world().tick();
    assert_eq!(
        &*responses(&mut host)[&41],
        expected(41, tick, Err(ErrorReason::InvalidEntity)).as_slice()
    );

    let mut stale_session = pick(42, output, viewport());
    stale_session[..8].copy_from_slice(&8u64.to_le_bytes());
    assert!(host.test_session().receive(&stale_session).is_err());
    assert!(host.test_session().session.pending.is_empty());
}

#[test]
fn output_replacement_before_query_frame_invalidates_selection_without_rebinding() {
    let mut host = ready();
    let output = camera(&mut host, 0.0);
    host.runtime_mut()
        .set_root_output(output, viewport())
        .unwrap();
    host.test_session()
        .receive(&pick(51, output, viewport()))
        .unwrap();
    let mut context = host.runtime_mut().world_mut(output.world().id()).unwrap();
    context
        .enqueue(Batch {
            id: 0,
            operations: vec![
                Command::RemoveComponent {
                    entity: EntityRef::Handle(output.camera_entity().unwrap()),
                    component: ComponentValue::CAMERA,
                },
                Command::insert_value(
                    EntityRef::Handle(output.camera_entity().unwrap()),
                    ComponentValue::Camera(Camera::default()),
                ),
            ],
        })
        .unwrap();
    drop(context);
    host.tick(0.0).unwrap();
    let tick = host.test_session().world().tick();
    assert_eq!(
        &*responses(&mut host)[&51],
        expected(51, tick, Err(ErrorReason::InvalidEntity)).as_slice()
    );
}

#[test]
fn failed_frame_completes_queries_once_without_losing_queued_batches_or_healthy_peers() {
    let failure = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(u64::MAX));
    let mut factories = ipp_core::systems::compiled_system_factories();
    factories.push(std::sync::Arc::new(FrameCheckFailure(failure.clone())));
    let mut host = Host::<CameraPlatform>::with_system_factories(factories).unwrap();
    host.open_session(
        7,
        &[
            crate::host::TEST_CAMERA_SYSTEMS,
            &[ipp_core::systems::SystemId("test.frame-check-failure")],
        ]
        .concat(),
    )
    .unwrap();
    host.test_session()
        .receive(&ipp_protocol::bootstrap())
        .unwrap();
    host.test_session().take_response().unwrap();
    let output = camera(&mut host, 0.0);
    host.runtime_mut()
        .set_root_output(output, viewport())
        .unwrap();
    host.tick(0.0).unwrap();
    responses(&mut host);
    let tick = host.test_session().world().tick();

    host.open_session(8, crate::host::TEST_CAMERA_SYSTEMS)
        .unwrap();
    host.session_mut(8)
        .unwrap()
        .receive(&ipp_protocol::bootstrap())
        .unwrap();
    host.session_mut(8).unwrap().take_response().unwrap();
    let create = |session: u64, request: u64| {
        let mut bytes = session.to_le_bytes().to_vec();
        bytes.extend(request.to_le_bytes());
        bytes.push(1);
        bytes.extend(99u32.to_le_bytes());
        bytes.push(1);
        bytes.extend(1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend(1u32.to_le_bytes());
        bytes.push(0);
        bytes.extend(0u32.to_le_bytes());
        bytes.push(0);
        bytes
    };
    host.session_mut(7)
        .unwrap()
        .receive(&create(7, 60))
        .unwrap();
    host.session_mut(7)
        .unwrap()
        .receive(&pick(61, output, viewport()))
        .unwrap();
    host.session_mut(8)
        .unwrap()
        .receive(&create(8, 70))
        .unwrap();
    failure.store(output.world().id().0, std::sync::atomic::Ordering::Relaxed);
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    let mut session = host.session_mut(7).unwrap();
    assert_eq!(
        &*session.take_response().unwrap(),
        expected(61, tick, Err(ErrorReason::Capacity)).as_slice()
    );
    assert_eq!(session.take_response().unwrap()[24], 22);
    assert!(session.take_response().is_none());
    assert_eq!(session.world().entities().len(), 1);
    assert_eq!(session.session.replies.len(), 1);
    assert!(session.session.prepared);
    drop(session);
    let peer = host.session_mut(8).unwrap().take_response().unwrap();
    assert_eq!(u64::from_le_bytes(peer[8..16].try_into().unwrap()), 70);
    assert_eq!(peer[24], 1);

    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    assert!(host.session_mut(7).unwrap().take_response().is_none());
    failure.store(u64::MAX, std::sync::atomic::Ordering::Relaxed);
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    let mut session = host.session_mut(7).unwrap();
    let reply = session.take_response().unwrap();
    assert_eq!(u64::from_le_bytes(reply[8..16].try_into().unwrap()), 60);
    assert_eq!(reply[24], 1);
    assert_eq!(session.world().entities().len(), 2);
    assert!(!session.session.prepared);
    while session.take_response().is_some() {}
    session.receive(&pick(62, output, viewport())).unwrap();
    let mut projection = pick(63, output, viewport());
    projection[16] = 12;
    projection.pop();
    for value in [0.0f32, 0.0, 0.0, 0.0, 0.0, 1.0] {
        projection.extend(value.to_le_bytes());
    }
    session.receive(&projection).unwrap();
    drop(session);
    failure.store(output.world().id().0, std::sync::atomic::Ordering::Relaxed);
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    let mut session = host.session_mut(7).unwrap();
    assert_eq!(
        &*session.take_response().unwrap(),
        expected(62, session.world().tick(), Err(ErrorReason::Capacity)).as_slice()
    );
    assert_eq!(
        &*session.take_response().unwrap(),
        ipp_protocol::encode_response(&Response {
            session: 7,
            request_id: 63,
            tick: session.world().tick(),
            body: ResponseBody::CameraProjectResultEvent(ipp_protocol::views::ViewQueryOutcome {
                request_id: 63,
                tick: session.world().tick(),
                result: Err(ErrorReason::Capacity),
            }),
        })
        .unwrap()
        .as_slice()
    );
    assert!(!session.session.prepared);
    assert!(session.session.request_origins.is_empty());
}
