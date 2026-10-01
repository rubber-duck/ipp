use super::*;
use ipp_core::{Batch, Command, ComponentValue, EntityRef, OutputKind, OutputRef, WorldViewport};
use ipp_protocol::host::{
    HOST_RESPONSE_MAGIC, HostRequest, HostRequestBody, HostResponse, HostResponseBody,
    decode_host_response, encode_host_request,
};

struct Platform {
    input: super::super::gui_input::GuiHostInputService,
    surface: PresentationSurface,
    draws: Vec<OutputRef>,
    lost_during_draw: bool,
    prepared: Vec<Option<OutputRef>>,
}

impl HostServices for Platform {
    const NAME: &'static str = "presentation-coordinator-test";

    fn initialize(_: &mut HostRuntime) -> Result<Self, String> {
        Ok(Self {
            input: Default::default(),
            surface: PresentationSurface {
                id: 1,
                context: 1,
                max_width: 4096,
                max_height: 4096,
            },
            draws: Vec::new(),
            lost_during_draw: false,
            prepared: Vec::new(),
        })
    }

    fn gui_input(&mut self) -> Option<&mut super::super::gui_input::GuiHostInputService> {
        Some(&mut self.input)
    }

    fn service_resources(&mut self, _: &mut HostRuntime) -> Result<(), String> {
        Ok(())
    }

    fn presentation_surface(&self) -> Result<PresentationSurface, PresentationError> {
        Ok(self.surface)
    }

    fn configure_presentation(&mut self, _: WorldViewport) -> Result<(), PresentationError> {
        Ok(())
    }

    fn prepare_presentation(
        &mut self,
        _: &mut HostRuntime,
        selected: Option<(OutputRef, ipp_core::WorldPublicationId)>,
    ) -> Result<(), HostPresentationFailure> {
        self.prepared.push(selected.map(|(output, _)| output));
        Ok(())
    }

    fn present(
        &mut self,
        _: &HostRuntime,
        output: OutputRef,
        publication: ipp_core::WorldPublicationId,
        _: WorldViewport,
        _: f64,
        completion: crate::PresentationCompletion<'_>,
    ) -> Result<PresentationDrawSummary, HostPresentationFailure> {
        let crate::PresentationCompletion {
            capture,
            outputs,
        } = completion;
        self.draws.push(output);
        if let Some(pixels) = capture {
            pixels.fill(output.world().id().0 as u8);
        }
        if self.lost_during_draw {
            self.surface.context += 1;
        }
        for observed in outputs {
            if observed.output == output {
                observed.publication = Some(publication);
            }
        }
        Ok(PresentationDrawSummary::default())
    }
}

#[path = "gui_input_tests.rs"]
mod gui_input_tests;

/// Apply one batch through the ordinary queue at a Host frame and return its created aliases.
fn apply(host: &mut HostRuntime, world: WorldId, batch: Batch) -> Vec<(u32, ipp_core::EntityId)> {
    host.world_mut(world).unwrap().enqueue(batch).unwrap();

    host.frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap()
}

fn root(host: &mut HostRuntime) -> RootBinding {
    let world = host
        .create_world(Default::default(), crate::host::TEST_CAMERA_SYSTEMS)
        .unwrap();
    let reference = host.world_ref(world).unwrap();
    let entity = apply(
        host,
        world,
        Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::insert_value(
                    EntityRef::Alias(1),
                    ComponentValue::Camera(Default::default()),
                ),
            ],
        },
    )[0]
    .1;

    let output = host
        .bind_output(reference, entity, OutputKind::Camera)
        .unwrap();
    host.set_root_output(
        output,
        WorldViewport {
            width: 2,
            height: 3,
            device_pixel_ratio: 1.0,
        },
    )
    .unwrap();
    host.root_output_binding(reference).unwrap().unwrap().into()
}

fn select(
    coordinator: &mut PresentationCoordinator,
    host: &mut HostRuntime,
    platform: &mut Platform,
    root: RootBinding,
) -> PresentationView {
    let Some(PresentationResponse::View(view)) = coordinator.request(
        host,
        platform,
        7,
        1,
        PresentationRequest::Select {
            surface: platform.surface,
            binding: root,
        },
        Duration::ZERO,
    ) else {
        panic!("selection must succeed")
    };
    view
}

fn queue(
    coordinator: &mut PresentationCoordinator,
    host: &mut HostRuntime,
    platform: &mut Platform,
    view: PresentationView,
    id: u64,
    capture: bool,
) {
    assert_eq!(
        coordinator.request(
            host,
            platform,
            7,
            id,
            PresentationRequest::Frame {
                view,
                after_sequence: None,
                publication: None,
                capture,
                after_outputs: Vec::new(),
            },
            Duration::ZERO
        ),
        None
    );
}

#[test]
fn only_explicit_selected_surface_draws_and_config_survives_connection_loss() {
    let mut host = HostRuntime::new();
    let first = root(&mut host);
    let second = root(&mut host);
    host.frame(0.0).unwrap();
    let mut platform = Platform::initialize(&mut host).unwrap();
    let mut coordinator = PresentationCoordinator::default();
    coordinator.draw(&mut host, &mut platform, 0.0, Duration::ZERO);
    assert!(platform.draws.is_empty());
    let view = select(&mut coordinator, &mut host, &mut platform, second);
    queue(&mut coordinator, &mut host, &mut platform, view, 2, true);
    coordinator.disconnect(7);
    coordinator.draw(&mut host, &mut platform, 0.0, Duration::ZERO);
    assert_eq!(platform.draws, [second.output.resolve(&host).unwrap()]);
    assert_eq!(coordinator.selected, Some(view));
    assert!(coordinator.take_completed().is_empty());
    assert!(
        host.root_output_binding(first.output.world.resolve(&host).unwrap())
            .unwrap()
            .is_some()
    );
}

#[test]
fn inclusion_cut_is_admitted_once_and_completes_at_the_next_evaluation() {
    let mut host = HostRuntime::new();
    let binding = root(&mut host);
    host.frame(0.0).unwrap();
    let mut platform = Platform::initialize(&mut host).unwrap();
    let mut coordinator = PresentationCoordinator::default();
    let view = select(&mut coordinator, &mut host, &mut platform, binding);
    let output = binding.output.resolve(&host).unwrap();
    let floor = host.output_evaluation_cut(output).unwrap();
    assert_eq!(
        coordinator.request(
            &mut host,
            &mut platform,
            7,
            2,
            PresentationRequest::Frame {
                view,
                after_sequence: None,
                publication: None,
                capture: false,
                after_outputs: vec![binding.output; 129],
            },
            Duration::ZERO
        ),
        None
    );
    assert_eq!(coordinator.pending[&(7, 2)].outputs, [(output, floor)]);
    coordinator.draw(&mut host, &mut platform, 0.0, Duration::ZERO);
    assert!(coordinator.completed.is_empty());
    assert_eq!(coordinator.pending[&(7, 2)].outputs, [(output, floor)]);
    host.frame(0.0).unwrap();
    coordinator.draw(&mut host, &mut platform, 0.0, Duration::ZERO);
    let [(7, 2, PresentationResponse::Frame(frame))] = coordinator.completed.as_slice() else {
        panic!("next authorized draw must satisfy unchanged output")
    };
    assert_eq!(frame.sources.len(), 1);
    assert_eq!(frame.sources[0].output, binding.output);
    assert_eq!(frame.sources[0].minimum_tick, floor);
    assert_eq!(frame.sources[0].tick, floor);
    assert_eq!(frame.sources[0].publication, frame.publication);
    assert!(coordinator.pending.is_empty());
}

#[test]
fn inclusion_admission_never_retargets_a_replaced_output() {
    let mut host = HostRuntime::new();
    let binding = root(&mut host);
    let child = root(&mut host);
    host.frame(0.0).unwrap();
    let mut platform = Platform::initialize(&mut host).unwrap();
    let mut coordinator = PresentationCoordinator::default();
    let view = select(&mut coordinator, &mut host, &mut platform, binding);
    assert_eq!(
        coordinator.request(
            &mut host,
            &mut platform,
            7,
            2,
            PresentationRequest::Frame {
                view,
                after_sequence: None,
                publication: None,
                capture: false,
                after_outputs: vec![child.output],
            },
            Duration::ZERO
        ),
        None
    );
    let world = child.output.world.resolve(&host).unwrap();
    let entity = child
        .output
        .resolve(&host)
        .unwrap()
        .camera_entity()
        .unwrap();
    apply(
        &mut host,
        world.id(),
        Batch {
            id: 2,
            operations: vec![
                Command::RemoveComponent {
                    entity: EntityRef::Handle(entity),
                    component: ComponentValue::CAMERA,
                },
                Command::insert_value(
                    EntityRef::Handle(entity),
                    ComponentValue::Camera(Default::default()),
                ),
            ],
        },
    );

    let replacement = host.bind_output(world, entity, OutputKind::Camera).unwrap();
    assert_ne!(
        ipp_protocol::references::OutputReference::from(replacement),
        child.output
    );
    coordinator.draw(&mut host, &mut platform, 0.0, Duration::ZERO);
    assert_eq!(
        coordinator.completed,
        [(
            7,
            2,
            PresentationResponse::Error(PresentationError::Unavailable)
        )]
    );
    assert!(coordinator.pending.is_empty());
}

#[test]
fn inclusion_accepts_more_than_sixty_four_exact_targets_and_releases_them_on_cancel() {
    let mut host = HostRuntime::new();
    let binding = root(&mut host);
    let outputs: Vec<_> = (0..65).map(|_| root(&mut host).output).collect();
    let mut platform = Platform::initialize(&mut host).unwrap();
    let mut coordinator = PresentationCoordinator::default();
    let view = select(&mut coordinator, &mut host, &mut platform, binding);
    assert_eq!(
        coordinator.request(
            &mut host,
            &mut platform,
            7,
            2,
            PresentationRequest::Frame {
                view,
                after_sequence: None,
                publication: None,
                capture: false,
                after_outputs: outputs,
            },
            Duration::ZERO
        ),
        None
    );
    assert_eq!(coordinator.pending[&(7, 2)].outputs.len(), 65);
    assert_eq!(
        coordinator.request(
            &mut host,
            &mut platform,
            7,
            3,
            PresentationRequest::CancelFrame {
                request: 2
            },
            Duration::ZERO
        ),
        Some(PresentationResponse::Complete)
    );
    assert!(coordinator.pending.is_empty());
    assert_eq!(
        coordinator.take_completed(),
        [(
            7,
            2,
            PresentationResponse::Error(PresentationError::Unavailable)
        )]
    );
    assert!(coordinator.take_completed().is_empty());
}

#[test]
fn inclusion_metadata_charge_transfers_to_the_actual_reply_lease() {
    use ipp_core::services::reliable_output::{OutputCharge, OutputStatus};
    let (mut host, view, session, _) = connected_surface();
    let account = host.sessions[&session].reply_budget.0.clone();
    control(
        &mut host,
        3,
        HostRequestBody::Presentation(PresentationRequest::Frame {
            view,
            after_sequence: None,
            publication: None,
            capture: false,
            after_outputs: vec![view.binding.output; 129],
        }),
    );
    assert_eq!(account.usage().entries, 1);
    assert!(account.usage().bytes >= 129 * 33);
    assert!(host.process_host_requests().is_empty());
    let reserved = account.usage();
    assert!(reserved.bytes >= 1024 + 129 * std::mem::size_of::<PresentedSource>());
    assert_eq!(host.presentation.pending[&(7, 3)].outputs.len(), 1);
    assert_eq!(account.usage(), reserved);
    host.tick_worlds(0.0).unwrap();
    let reply = host.take_connection_response(7).unwrap();
    let decoded = decode_host_response(&reply, 7).unwrap();
    assert!(
        matches!(decoded.body, HostResponseBody::Presentation(PresentationResponse::Frame(ref frame)) if frame.sources.len() == 1)
    );
    assert!(host.presentation.pending.is_empty());
    let (bytes, lease) = reply.into_parts();
    while let Some(progress) = host.take_connection_response(7) {
        assert!(!progress.starts_with(HOST_RESPONSE_MAGIC));
    }
    let retained = account.usage();
    assert_eq!(retained.entries, 1);
    assert_eq!(
        retained.bytes,
        lease.capacity() + crate::reliable_output::RESPONSE_METADATA_BYTES
    );
    drop(bytes);
    host.close_connection(7);
    assert_eq!(account.status(), OutputStatus::Closed);
    assert_eq!(account.usage(), retained);
    drop(lease);
    assert_eq!(account.usage(), OutputCharge::default());
}

#[test]
fn rebind_cycle_and_stale_clear_do_not_relabel_pending_captures() {
    let mut host = HostRuntime::new();
    let binding = root(&mut host);
    host.frame(0.0).unwrap();
    let mut platform = Platform::initialize(&mut host).unwrap();
    let mut coordinator = PresentationCoordinator::default();
    let old = select(&mut coordinator, &mut host, &mut platform, binding);
    queue(&mut coordinator, &mut host, &mut platform, old, 2, true);
    let replacement = select(&mut coordinator, &mut host, &mut platform, binding);
    assert_ne!(replacement.selection, old.selection);
    coordinator.request(
        &mut host,
        &mut platform,
        7,
        3,
        PresentationRequest::Clear(old),
        Duration::ZERO,
    );
    assert_eq!(coordinator.selected, Some(replacement));
    coordinator.draw(&mut host, &mut platform, 0.0, Duration::ZERO);
    assert_eq!(
        coordinator.take_completed(),
        [(
            7,
            2,
            PresentationResponse::Error(PresentationError::StaleView)
        )]
    );
    queue(
        &mut coordinator,
        &mut host,
        &mut platform,
        replacement,
        4,
        true,
    );
    host.set_root_output(binding.output.resolve(&host).unwrap(), binding.viewport)
        .unwrap();
    coordinator.draw(&mut host, &mut platform, 0.0, Duration::ZERO);
    assert_eq!(
        coordinator.take_completed(),
        [(
            7,
            4,
            PresentationResponse::Error(PresentationError::StaleView)
        )]
    );
}

#[test]
fn context_loss_during_readback_fences_completion_but_completed_bytes_survive_loss() {
    let mut host = HostRuntime::new();
    let binding = root(&mut host);
    host.frame(0.0).unwrap();
    let mut platform = Platform::initialize(&mut host).unwrap();
    let mut coordinator = PresentationCoordinator::default();
    let view = select(&mut coordinator, &mut host, &mut platform, binding);
    queue(&mut coordinator, &mut host, &mut platform, view, 2, true);
    coordinator.draw(&mut host, &mut platform, 0.0, Duration::ZERO);
    let completed = coordinator.take_completed();
    let PresentationResponse::Capture {
        ref frame,
        capture,
        bytes,
    } = completed[0].2
    else {
        panic!("capture")
    };
    assert_eq!(frame.view, view);
    assert_eq!(bytes, 24);
    queue(&mut coordinator, &mut host, &mut platform, view, 3, true);
    platform.lost_during_draw = true;
    coordinator.draw(&mut host, &mut platform, 0.0, Duration::ZERO);
    assert_eq!(
        coordinator.take_completed(),
        [(
            7,
            3,
            PresentationResponse::Error(PresentationError::StaleView)
        )]
    );
    assert_eq!(
        coordinator.request(
            &mut host,
            &mut platform,
            7,
            4,
            PresentationRequest::ReadCapture {
                capture,
                offset: 0
            },
            Duration::ZERO
        ),
        Some(PresentationResponse::Chunk {
            capture,
            offset: 0,
            bytes: vec![binding.output.world.id as u8; 24]
        })
    );
    assert_eq!(
        coordinator.request(
            &mut host,
            &mut platform,
            8,
            1,
            PresentationRequest::ReadCapture {
                capture,
                offset: 0
            },
            Duration::ZERO
        ),
        Some(PresentationResponse::Error(PresentationError::Unavailable))
    );
    coordinator.expire(CAPTURE_TIMEOUT);
    assert!(coordinator.captures.is_empty());
}

#[test]
fn exact_publication_never_replays_history_and_future_sequence_expires() {
    let mut host = HostRuntime::new();
    let binding = root(&mut host);
    host.frame(0.0).unwrap();
    let mut platform = Platform::initialize(&mut host).unwrap();
    let mut coordinator = PresentationCoordinator::default();
    let view = select(&mut coordinator, &mut host, &mut platform, binding);
    coordinator.request(
        &mut host,
        &mut platform,
        7,
        2,
        PresentationRequest::Frame {
            view,
            after_sequence: None,
            publication: Some(PresentationIdentity {
                host: 0,
                serial: 0,
            }),
            capture: true,
            after_outputs: Vec::new(),
        },
        Duration::ZERO,
    );
    coordinator.draw(&mut host, &mut platform, 0.0, Duration::ZERO);
    assert_eq!(
        coordinator.take_completed(),
        [(
            7,
            2,
            PresentationResponse::Error(PresentationError::ObsoletePublication)
        )]
    );
    coordinator.request(
        &mut host,
        &mut platform,
        7,
        3,
        PresentationRequest::Frame {
            view,
            after_sequence: Some(u64::MAX),
            publication: None,
            capture: true,
            after_outputs: Vec::new(),
        },
        Duration::ZERO,
    );
    coordinator.expire(FRAME_TIMEOUT);
    assert_eq!(
        coordinator.take_completed(),
        [(
            7,
            3,
            PresentationResponse::Error(PresentationError::Timeout)
        )]
    );
    assert_eq!(coordinator.used_bytes(), 0);
}

#[test]
fn viewport_is_exact_and_pending_and_transfer_budgets_are_bounded() {
    let mut host = HostRuntime::new();
    let binding = root(&mut host);
    let mut platform = Platform::initialize(&mut host).unwrap();
    let mut coordinator = PresentationCoordinator::default();
    platform.surface.max_height = 2;
    let surface = platform.surface;
    assert_eq!(
        coordinator.request(
            &mut host,
            &mut platform,
            7,
            1,
            PresentationRequest::Select {
                surface,
                binding
            },
            Duration::ZERO
        ),
        Some(PresentationResponse::Error(
            PresentationError::InvalidViewport
        ))
    );
    assert!(coordinator.selected.is_none());
    platform.surface.max_height = 4096;
    let view = select(&mut coordinator, &mut host, &mut platform, binding);
    for request in 2..6 {
        queue(
            &mut coordinator,
            &mut host,
            &mut platform,
            view,
            request,
            true,
        );
    }
    assert_eq!(
        coordinator.request(
            &mut host,
            &mut platform,
            7,
            6,
            PresentationRequest::Frame {
                view,
                after_sequence: None,
                publication: None,
                capture: true,
                after_outputs: Vec::new(),
            },
            Duration::ZERO
        ),
        Some(PresentationResponse::Error(PresentationError::Capacity))
    );
    assert_eq!(coordinator.used_bytes(), 4 * 24);
}

fn frame_for(
    coordinator: &mut PresentationCoordinator,
    host: &mut HostRuntime,
    platform: &mut Platform,
    view: PresentationView,
    (connection, id): (u64, u64),
    capture: bool,
) -> Option<PresentationResponse> {
    coordinator.request(
        host,
        platform,
        connection,
        id,
        PresentationRequest::Frame {
            view,
            after_sequence: None,
            publication: None,
            capture,
            after_outputs: Vec::new(),
        },
        Duration::ZERO,
    )
}

#[test]
fn a_connection_at_its_presentation_allowance_leaves_others_theirs() {
    let mut host = HostRuntime::new();
    let binding = root(&mut host);
    let mut platform = Platform::initialize(&mut host).unwrap();
    let mut coordinator = PresentationCoordinator::default();
    let view = select(&mut coordinator, &mut host, &mut platform, binding);
    let capacity = Some(PresentationResponse::Error(PresentationError::Capacity));

    // Every presenting connection but one reaches its own waiter allowance.
    for connection in 1..PRESENTING_CONNECTIONS as u64 {
        for id in 0..PER_CONNECTION as u64 {
            let queued = (connection, 100 + id);
            let reply = frame_for(
                &mut coordinator,
                &mut host,
                &mut platform,
                view,
                queued,
                true,
            );
            assert_eq!(reply, None);
        }
        let over = (connection, 200);
        let reply = frame_for(
            &mut coordinator,
            &mut host,
            &mut platform,
            view,
            over,
            false,
        );
        assert_eq!(reply, capacity, "over its own allowance");
    }

    // Those allowances never cause Capacity for another connection.
    let last = PRESENTING_CONNECTIONS as u64;
    for id in 0..PER_CONNECTION as u64 {
        let queued = (last, 100 + id);
        let reply = frame_for(
            &mut coordinator,
            &mut host,
            &mut platform,
            view,
            queued,
            true,
        );
        assert_eq!(reply, None);
    }
    assert_eq!(coordinator.pending.len(), MAX_REQUESTS);
}

#[test]
fn capture_bytes_are_bounded_per_connection_before_the_host_pool() {
    let mut host = HostRuntime::new();
    let small = root(&mut host);
    let output = small.output.resolve(&host).unwrap();
    host.set_root_output(
        output,
        WorldViewport {
            width: 4096,
            height: 4096,
            device_pixel_ratio: 1.0,
        },
    )
    .unwrap();
    let binding = host
        .root_output_binding(output.world())
        .unwrap()
        .unwrap()
        .into();
    let mut platform = Platform::initialize(&mut host).unwrap();
    let mut coordinator = PresentationCoordinator::default();
    let view = select(&mut coordinator, &mut host, &mut platform, binding);
    let capacity = Some(PresentationResponse::Error(PresentationError::Capacity));
    assert_eq!(4096 * 4096 * 4, PER_CONNECTION_CAPTURE_BYTES);

    // One maximum capture fills a connection's byte allowance; frames still fit.
    for connection in 1..=PRESENTING_CONNECTIONS as u64 {
        let first = (connection, 1);
        let reply = frame_for(
            &mut coordinator,
            &mut host,
            &mut platform,
            view,
            first,
            true,
        );
        assert_eq!(
            reply, None,
            "connection {connection} has its own capture bytes"
        );
        let second = (connection, 2);
        let reply = frame_for(
            &mut coordinator,
            &mut host,
            &mut platform,
            view,
            second,
            true,
        );
        assert_eq!(reply, capacity);
        let frame = (connection, 3);
        let reply = frame_for(
            &mut coordinator,
            &mut host,
            &mut platform,
            view,
            frame,
            false,
        );
        assert_eq!(reply, None);
    }
    assert_eq!(coordinator.used_bytes(), MAX_CAPTURE_BYTES);
}

fn control(host: &mut crate::Host<Platform>, request_id: u64, body: HostRequestBody) {
    let bytes = encode_host_request(&HostRequest {
        connection: 7,
        request_id,
        body,
    })
    .unwrap();
    host.receive_connection(7, &bytes).unwrap();
}

fn control_replies(host: &mut crate::Host<Platform>) -> Vec<HostResponse> {
    let mut replies = Vec::new();
    while let Some(bytes) = host.take_connection_response(7) {
        if bytes.starts_with(HOST_RESPONSE_MAGIC) {
            replies.push(decode_host_response(&bytes, 7).unwrap());
        }
    }
    replies
}

fn connected_surface() -> (crate::Host<Platform>, PresentationView, u64, WorldId) {
    let mut host = crate::Host::<Platform>::new().unwrap();
    let binding = root(host.runtime_mut());
    let peer = root(host.runtime_mut());
    host.tick_worlds(0.0).unwrap();
    host.open_connection(7).unwrap();
    host.receive_connection(7, &ipp_protocol::HELLO).unwrap();
    host.take_connection_response(7).unwrap();
    control(
        &mut host,
        1,
        HostRequestBody::OpenWorld(binding.output.world),
    );
    assert!(host.process_host_requests().is_empty());
    let replies = control_replies(&mut host);
    let HostResponseBody::Attached {
        session,
        ..
    } = replies[0].body
    else {
        panic!("World session")
    };
    let surface = host.services_mut().surface;
    control(
        &mut host,
        2,
        HostRequestBody::Presentation(PresentationRequest::Select {
            surface,
            binding,
        }),
    );
    assert!(host.process_host_requests().is_empty());
    let replies = control_replies(&mut host);
    let HostResponseBody::Presentation(PresentationResponse::View(view)) = replies[0].body else {
        panic!("view")
    };
    (host, view, session, WorldId(peer.output.world.id))
}

#[test]
fn connection_frame_and_capture_observe_current_publication_until_the_view_is_stale() {
    let (mut host, view, _, peer) = connected_surface();
    let world = view.binding.output.world.resolve(host.runtime()).unwrap();
    control_replies(&mut host);
    let tick = host.runtime_mut().world_mut(world.id()).unwrap().tick();
    let peer_tick = host.runtime_mut().world_mut(peer).unwrap().tick();
    for (request, capture) in [(4, false), (5, true)] {
        control(
            &mut host,
            request,
            HostRequestBody::Presentation(PresentationRequest::Frame {
                view,
                after_sequence: None,
                publication: None,
                capture,
                after_outputs: Vec::new(),
            }),
        );
    }
    assert!(host.process_host_requests().is_empty());
    assert!(control_replies(&mut host).is_empty());
    host.tick_worlds(0.0).unwrap();
    let replies = control_replies(&mut host);
    assert_eq!(
        replies
            .iter()
            .map(|reply| reply.request_id)
            .collect::<Vec<_>>(),
        [4, 5]
    );
    let (_, _, publication) = host.runtime().root_output(world.id()).unwrap();
    let (identity, revision) = publication.identity();
    for reply in &replies {
        let frame = match &reply.body {
            HostResponseBody::Presentation(PresentationResponse::Frame(frame)) => frame,
            HostResponseBody::Presentation(PresentationResponse::Capture {
                frame,
                ..
            }) => frame,
            _ => panic!("successful presentation expected"),
        };
        assert_eq!(frame.view, view);
        assert_eq!(
            frame.publication,
            PresentationIdentity {
                host: identity,
                serial: revision
            }
        );
    }
    assert_eq!(
        host.runtime_mut().world_mut(world.id()).unwrap().tick(),
        tick + 1
    );
    assert_eq!(
        host.runtime_mut().world_mut(peer).unwrap().tick(),
        peer_tick + 1
    );
    let output = view.binding.output.resolve(host.runtime()).unwrap();
    host.runtime_mut()
        .set_root_output(output, view.binding.viewport)
        .unwrap();
    control(
        &mut host,
        6,
        HostRequestBody::Presentation(PresentationRequest::Frame {
            view,
            after_sequence: None,
            publication: None,
            capture: true,
            after_outputs: Vec::new(),
        }),
    );
    host.tick_worlds(0.0).unwrap();
    assert_eq!(
        control_replies(&mut host),
        [HostResponse {
            connection: 7,
            request_id: 6,
            body: HostResponseBody::Presentation(PresentationResponse::Error(
                PresentationError::StaleView
            ))
        }]
    );
    let binding = host
        .runtime()
        .root_output_binding(world)
        .unwrap()
        .unwrap()
        .into();
    control(
        &mut host,
        7,
        HostRequestBody::Presentation(PresentationRequest::Select {
            surface: view.surface,
            binding,
        }),
    );
    assert!(host.process_host_requests().is_empty());
    let replies = control_replies(&mut host);
    let HostResponseBody::Presentation(PresentationResponse::View(replacement)) = replies[0].body
    else {
        panic!("new view")
    };
    assert_ne!(replacement.binding.generation, view.binding.generation);
    control(
        &mut host,
        8,
        HostRequestBody::Presentation(PresentationRequest::Clear(view)),
    );
    control(
        &mut host,
        9,
        HostRequestBody::Presentation(PresentationRequest::Frame {
            view: replacement,
            after_sequence: None,
            publication: None,
            capture: false,
            after_outputs: Vec::new(),
        }),
    );
    host.tick_worlds(0.0).unwrap();
    let replies = control_replies(&mut host);
    assert!(
        matches!(&replies[1].body, HostResponseBody::Presentation(PresentationResponse::Frame(frame)) if frame.view == replacement)
    );
    host.close_connection(7);
}

#[test]
fn completed_capture_transfers_answer_at_ingress_without_waiting_for_host_frames() {
    let (mut host, view, _, _) = connected_surface();
    control_replies(&mut host);
    control(
        &mut host,
        4,
        HostRequestBody::Presentation(PresentationRequest::Frame {
            view,
            after_sequence: None,
            publication: None,
            capture: true,
            after_outputs: Vec::new(),
        }),
    );
    host.tick_worlds(0.0).unwrap();
    let replies = control_replies(&mut host);
    let HostResponseBody::Presentation(PresentationResponse::Capture {
        capture,
        bytes,
        ..
    }) = replies[0].body
    else {
        panic!("capture")
    };
    let read = |offset| {
        HostRequestBody::Presentation(PresentationRequest::ReadCapture {
            capture,
            offset,
        })
    };

    // Every chunk and the release answer at ingress, between Host frames.
    let mut offset = 0;
    let mut request = 5;
    while offset < bytes {
        control(&mut host, request, read(offset));
        let replies = control_replies(&mut host);
        let [
            HostResponse {
                request_id,
                body:
                    HostResponseBody::Presentation(PresentationResponse::Chunk {
                        bytes,
                        ..
                    }),
                ..
            },
        ] = replies.as_slice()
        else {
            panic!("immediate chunk")
        };
        assert_eq!(*request_id, request);
        offset += bytes.len() as u64;
        request += 1;
    }
    control(
        &mut host,
        request,
        HostRequestBody::Presentation(PresentationRequest::ReleaseCapture(capture)),
    );
    assert_eq!(
        control_replies(&mut host),
        [HostResponse {
            connection: 7,
            request_id: request,
            body: HostResponseBody::Presentation(PresentationResponse::Complete),
        }]
    );

    // A transfer never waits behind control requests queued for the next frame.
    control(
        &mut host,
        20,
        HostRequestBody::Presentation(PresentationRequest::Frame {
            view,
            after_sequence: None,
            publication: None,
            capture: true,
            after_outputs: Vec::new(),
        }),
    );
    control(&mut host, 21, read(0));
    assert_eq!(
        control_replies(&mut host),
        [HostResponse {
            connection: 7,
            request_id: 21,
            body: HostResponseBody::Presentation(PresentationResponse::Error(
                PresentationError::Unavailable
            )),
        }]
    );
    host.tick_worlds(0.0).unwrap();
    assert!(matches!(
        control_replies(&mut host).as_slice(),
        [HostResponse {
            request_id: 20,
            body: HostResponseBody::Presentation(PresentationResponse::Capture { .. }),
            ..
        }]
    ));
    host.close_connection(7);
}

#[test]
fn connection_frame_deadline_starts_at_ingress_even_before_dispatch() {
    for dispatch in [false, true] {
        let (mut host, view, _, _) = connected_surface();
        let world = WorldId(view.binding.output.world.id);
        let tick = host.runtime_mut().world_mut(world).unwrap().tick();
        let draws = host.services_mut().draws.len();
        control(
            &mut host,
            3,
            HostRequestBody::Presentation(PresentationRequest::Frame {
                view,
                after_sequence: None,
                publication: None,
                capture: true,
                after_outputs: Vec::new(),
            }),
        );
        host.maintain_connections(Duration::from_secs(4));
        if dispatch {
            assert!(host.process_host_requests().is_empty());
        }
        assert!(control_replies(&mut host).is_empty());
        host.maintain_connections(FRAME_TIMEOUT);
        assert_eq!(
            control_replies(&mut host),
            [HostResponse {
                connection: 7,
                request_id: 3,
                body: HostResponseBody::Presentation(PresentationResponse::Error(
                    PresentationError::Timeout
                ))
            }]
        );
        assert_eq!(host.runtime_mut().world_mut(world).unwrap().tick(), tick);
        assert_eq!(host.services_mut().draws.len(), draws);
        assert!(host.process_host_requests().is_empty());
        assert!(control_replies(&mut host).is_empty());
    }
}

fn presentation_wire(connection: u64, request: u64, payload: &[u8]) -> Vec<u8> {
    let mut bytes = b"IPPH\x02\0\0\0".to_vec();
    bytes.extend_from_slice(&connection.to_le_bytes());
    bytes.extend_from_slice(&request.to_le_bytes());
    bytes.push(23);
    bytes.extend_from_slice(payload);
    bytes
}

fn cancellation_wire(connection: u64, request: u64, original: u64) -> Vec<u8> {
    let mut payload = vec![7];
    payload.extend_from_slice(&original.to_le_bytes());
    presentation_wire(connection, request, &payload)
}

fn frame_wire(view: PresentationView, connection: u64, request: u64, future: bool) -> Vec<u8> {
    let mut payload = vec![4];
    payload.extend_from_slice(&view.surface.id.to_le_bytes());
    payload.extend_from_slice(&view.surface.context.to_le_bytes());
    payload.extend_from_slice(&view.surface.max_width.to_le_bytes());
    payload.extend_from_slice(&view.surface.max_height.to_le_bytes());
    payload.extend_from_slice(&view.selection.to_le_bytes());
    let output = view.binding.output;
    let ipp_protocol::references::OutputTarget::Camera {
        entity,
        incarnation,
    } = output.target
    else {
        panic!("camera output");
    };
    payload.extend_from_slice(&output.world.id.to_le_bytes());
    payload.extend_from_slice(&output.world.incarnation.to_le_bytes());
    payload.push(1);
    payload.extend_from_slice(&entity.to_le_bytes());
    payload.extend_from_slice(&incarnation.to_le_bytes());
    payload.extend_from_slice(&view.binding.viewport.width.to_le_bytes());
    payload.extend_from_slice(&view.binding.viewport.height.to_le_bytes());
    payload.extend_from_slice(&view.binding.viewport.device_pixel_ratio.to_le_bytes());
    payload.extend_from_slice(&view.binding.generation.host.to_le_bytes());
    payload.extend_from_slice(&view.binding.generation.serial.to_le_bytes());
    payload.push(u8::from(future));
    if future {
        payload.extend_from_slice(&u64::MAX.to_le_bytes());
    }
    payload.extend_from_slice(&[0, 1]);
    payload.extend_from_slice(&0_u32.to_le_bytes());
    presentation_wire(connection, request, &payload)
}

fn presentation_reply(connection: u64, request: u64, payload: &[u8]) -> Vec<u8> {
    let mut bytes = b"IPPA\x02\0\0\0".to_vec();
    bytes.extend_from_slice(&connection.to_le_bytes());
    bytes.extend_from_slice(&request.to_le_bytes());
    bytes.push(17);
    bytes.extend_from_slice(payload);
    bytes
}

fn dispatch_wire(host: &mut crate::Host<Platform>, connection: u64, bytes: &[u8]) {
    host.receive_connection(connection, bytes).unwrap();
    assert!(host.process_host_requests().is_empty());
}

fn terminal_wire(host: &mut crate::Host<Platform>, connection: u64, expected: &[u8]) {
    let reply = host
        .take_connection_response(connection)
        .expect("wire terminal");
    assert_eq!(&*reply, expected);
    assert!(host.take_connection_response(connection).is_none());
}

fn peer_connection(host: &mut crate::Host<Platform>) {
    host.open_connection(8).unwrap();
    host.receive_connection(8, &ipp_protocol::HELLO).unwrap();
    drop(host.take_connection_response(8).unwrap());
}

#[test]
fn connection_cancel_wire_correlations_and_completion_leases() {
    use ipp_core::services::reliable_output::{OutputCharge, OutputStatus};

    let (mut host, view, session, _) = connected_surface();
    let account = host.sessions[&session].reply_budget.0.clone();
    assert_eq!(account.usage(), OutputCharge::default());
    let world = WorldId(view.binding.output.world.id);
    let tick = host.runtime_mut().world_mut(world).unwrap().tick();
    let draws = host.services_mut().draws.len();
    let original = frame_wire(view, 7, 1000, true);
    dispatch_wire(&mut host, 7, &original);
    assert!(host.take_connection_response(7).is_none());
    let reserved_capture_bytes = host.presentation.used_bytes();
    assert_eq!(reserved_capture_bytes, 24);
    assert_eq!(account.usage().entries, 1);

    let case = std::env::var_os("IPP_PRESENTATION_WIRE_CASE").map(std::path::PathBuf::from);
    let expected_cancel = cancellation_wire(7, 1001, 1000);
    let cancel = case.as_ref().map_or_else(
        || expected_cancel.clone(),
        |directory| std::fs::read(directory.join("request.bin")).unwrap(),
    );
    assert_eq!(cancel, expected_cancel);
    dispatch_wire(&mut host, 7, &cancel);
    let remaining_capture_bytes = host.presentation.used_bytes();
    assert_eq!(remaining_capture_bytes, 0);
    assert!(host.presentation.pending.is_empty());
    assert_eq!(account.usage().entries, 2);
    let cancelled = host.take_connection_response(7).unwrap();
    assert_eq!(&*cancelled, presentation_reply(7, 1001, &[6]));
    assert!(host.take_connection_response(7).is_none());
    host.maintain_connections(Duration::ZERO);
    let unavailable = host.take_connection_response(7).unwrap();
    assert_eq!(&*unavailable, presentation_reply(7, 1000, &[7, 2]));
    host.maintain_connections(Duration::ZERO);
    assert!(host.take_connection_response(7).is_none());
    assert_eq!(host.runtime_mut().world_mut(world).unwrap().tick(), tick);
    assert_eq!(host.services_mut().draws.len(), draws);

    let cancelled_wire = cancelled.to_vec();
    let unavailable_wire = unavailable.to_vec();
    let (cancel_bytes, cancel_lease) = cancelled.into_parts();
    let (original_bytes, original_lease) = unavailable.into_parts();
    let retained = OutputCharge {
        entries: 2,
        bytes: cancel_lease.capacity()
            + original_lease.capacity()
            + 2 * crate::reliable_output::RESPONSE_METADATA_BYTES,
    };
    assert_eq!(account.usage(), retained);
    let mut charges = vec![account.usage()];
    drop(cancel_bytes);
    drop(original_bytes);
    assert_eq!(account.usage(), retained);
    charges.push(account.usage());
    assert!(host.close_connection(7));
    assert_eq!(account.status(), OutputStatus::Closed);
    assert_eq!(account.usage(), retained);
    charges.push(account.usage());
    let cancel_charge = cancel_lease.capacity() + crate::reliable_output::RESPONSE_METADATA_BYTES;
    drop(cancel_lease);
    assert_eq!(
        account.usage(),
        OutputCharge {
            entries: 1,
            bytes: retained.bytes - cancel_charge
        }
    );
    charges.push(account.usage());
    drop(original_lease);
    assert_eq!(account.usage(), OutputCharge::default());
    charges.push(account.usage());

    if let Some(directory) = case {
        let identity = std::fs::read_to_string(directory.join("identity.json")).unwrap();
        let output = view.binding.output;
        let ipp_protocol::references::OutputTarget::Camera {
            entity,
            incarnation,
        } = output.target
        else {
            panic!("camera output");
        };
        let fields = [
            view.surface.id,
            view.surface.context,
            u64::from(view.surface.max_width),
            u64::from(view.surface.max_height),
            view.selection,
            output.world.id,
            output.world.incarnation,
            entity,
            incarnation,
            u64::from(view.binding.viewport.width),
            u64::from(view.binding.viewport.height),
            view.binding.generation.host,
            view.binding.generation.serial,
        ]
        .map(|value| value.to_string());
        let entries: Vec<_> = charges.iter().map(|charge| charge.entries).collect();
        let bytes: Vec<_> = charges.iter().map(|charge| charge.bytes).collect();
        let transcript = format!(
            "{{\"identity\":{identity},\"schemaHash\":\"{}\",\"view\":{fields:?},\"frame\":{original:?},\"cancel\":{cancel:?},\"complete\":{cancelled_wire:?},\"unavailable\":{unavailable_wire:?},\"captureReservation\":[{reserved_capture_bytes},{remaining_capture_bytes}],\"replyEntries\":{entries:?},\"replyBytes\":{bytes:?}}}\n",
            ipp_protocol::schema_hash()
        );
        std::fs::write(directory.join("transcript.json"), transcript).unwrap();
    }
}

#[test]
fn connection_cancel_isolated_duplicate_and_unknown_requests() {
    let (mut host, view, _, _) = connected_surface();
    peer_connection(&mut host);
    for connection in [7, 8] {
        dispatch_wire(
            &mut host,
            connection,
            &frame_wire(view, connection, 1000, true),
        );
    }
    assert_eq!(host.presentation.used_bytes(), 48);
    dispatch_wire(&mut host, 8, &cancellation_wire(8, 1001, 1000));
    terminal_wire(&mut host, 8, &presentation_reply(8, 1001, &[6]));
    host.maintain_connections(Duration::ZERO);
    terminal_wire(&mut host, 8, &presentation_reply(8, 1000, &[7, 2]));
    assert!(host.take_connection_response(7).is_none());
    assert!(host.presentation.pending.contains_key(&(7, 1000)));
    assert_eq!(host.presentation.used_bytes(), 24);

    for (request, original) in [(1002, 1000), (1003, 9999)] {
        dispatch_wire(&mut host, 8, &cancellation_wire(8, request, original));
        terminal_wire(&mut host, 8, &presentation_reply(8, request, &[6]));
        host.maintain_connections(Duration::ZERO);
        assert!(host.take_connection_response(8).is_none());
        assert!(host.take_connection_response(7).is_none());
        assert_eq!(host.presentation.used_bytes(), 24);
    }
    dispatch_wire(&mut host, 7, &cancellation_wire(7, 1001, 1000));
    terminal_wire(&mut host, 7, &presentation_reply(7, 1001, &[6]));
    host.maintain_connections(Duration::ZERO);
    terminal_wire(&mut host, 7, &presentation_reply(7, 1000, &[7, 2]));
    host.maintain_connections(FRAME_TIMEOUT);
    assert!(host.take_connection_response(7).is_none());
    assert!(host.take_connection_response(8).is_none());
    assert_eq!(host.presentation.used_bytes(), 0);
    assert!(host.close_connection(7));
    assert!(host.close_connection(8));
}

#[test]
fn connection_cancel_releases_pending_capture_bytes_and_slots() {
    for large in [false, true] {
        let (mut host, mut view, _, _) = connected_surface();
        let (count, bytes) = if large {
            (2, PER_CONNECTION_CAPTURE_BYTES / 2)
        } else {
            (PER_CONNECTION, 24)
        };
        if large {
            let output = view.binding.output.resolve(host.runtime()).unwrap();
            host.runtime_mut()
                .set_root_output(
                    output,
                    WorldViewport {
                        width: 4096,
                        height: 2048,
                        device_pixel_ratio: 1.0,
                    },
                )
                .unwrap();
            let binding = host
                .runtime()
                .root_output_binding(output.world())
                .unwrap()
                .unwrap()
                .into();
            control(
                &mut host,
                3,
                HostRequestBody::Presentation(PresentationRequest::Select {
                    surface: view.surface,
                    binding,
                }),
            );
            assert!(host.process_host_requests().is_empty());
            let replies = control_replies(&mut host);
            let HostResponseBody::Presentation(PresentationResponse::View(selected)) =
                replies[0].body
            else {
                panic!("replacement view")
            };
            view = selected;
        }
        for index in 0..count {
            dispatch_wire(
                &mut host,
                7,
                &frame_wire(view, 7, 1000 + index as u64, true),
            );
        }
        assert_eq!(host.presentation.used_bytes(), count * bytes);
        dispatch_wire(&mut host, 7, &frame_wire(view, 7, 2000, true));
        terminal_wire(&mut host, 7, &presentation_reply(7, 2000, &[7, 6]));
        dispatch_wire(&mut host, 7, &cancellation_wire(7, 2001, 1000));
        terminal_wire(&mut host, 7, &presentation_reply(7, 2001, &[6]));
        assert_eq!(host.presentation.used_bytes(), (count - 1) * bytes);
        host.maintain_connections(Duration::ZERO);
        terminal_wire(&mut host, 7, &presentation_reply(7, 1000, &[7, 2]));
        dispatch_wire(&mut host, 7, &frame_wire(view, 7, 2002, true));
        assert!(host.take_connection_response(7).is_none());
        assert_eq!(host.presentation.pending.len(), count);
        assert_eq!(host.presentation.used_bytes(), count * bytes);
        assert!(host.close_connection(7));
        assert_eq!(host.presentation.used_bytes(), 0);
    }
}

#[test]
fn connection_cancel_completed_capture_retains_connection_owned_bytes() {
    let (mut host, view, _, _) = connected_surface();
    peer_connection(&mut host);
    dispatch_wire(&mut host, 7, &frame_wire(view, 7, 1000, false));
    host.tick_worlds(0.0).unwrap();
    let replies = control_replies(&mut host);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].request_id, 1000);
    let HostResponseBody::Presentation(PresentationResponse::Capture {
        ref frame,
        capture,
        bytes,
    }) = replies[0].body
    else {
        panic!("completed capture")
    };
    assert_eq!(frame.view, view);
    assert_eq!(bytes, 24);
    assert_eq!(host.presentation.used_bytes(), 24);
    host.services_mut().surface.context += 1;

    for request in [1001, 1002] {
        dispatch_wire(&mut host, 7, &cancellation_wire(7, request, 1000));
        terminal_wire(&mut host, 7, &presentation_reply(7, request, &[6]));
        host.maintain_connections(Duration::ZERO);
        assert!(host.take_connection_response(7).is_none());
        assert_eq!(host.presentation.used_bytes(), 24);
    }
    let mut read = vec![5];
    read.extend_from_slice(&capture.to_le_bytes());
    read.extend_from_slice(&0_u64.to_le_bytes());
    dispatch_wire(&mut host, 8, &presentation_wire(8, 1003, &read));
    terminal_wire(&mut host, 8, &presentation_reply(8, 1003, &[7, 2]));
    dispatch_wire(&mut host, 7, &presentation_wire(7, 1003, &read));
    let mut chunk = read.clone();
    chunk.extend_from_slice(&24_u32.to_le_bytes());
    chunk.extend_from_slice(&[view.binding.output.world.id as u8; 24]);
    terminal_wire(&mut host, 7, &presentation_reply(7, 1003, &chunk));
    let mut release = vec![6];
    release.extend_from_slice(&capture.to_le_bytes());
    dispatch_wire(&mut host, 7, &presentation_wire(7, 1004, &release));
    terminal_wire(&mut host, 7, &presentation_reply(7, 1004, &[6]));
    assert_eq!(host.presentation.used_bytes(), 0);
    dispatch_wire(&mut host, 7, &presentation_wire(7, 1005, &read));
    terminal_wire(&mut host, 7, &presentation_reply(7, 1005, &[7, 2]));
    assert!(host.close_connection(7));
    assert!(host.close_connection(8));
}
