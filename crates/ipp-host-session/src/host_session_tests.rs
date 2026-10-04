use super::*;

struct TestPlatform;

impl HostServices for TestPlatform {
    const NAME: &'static str = "test";

    fn initialize(
        _world: &mut ipp_core::HostRuntime,
        _schedulers: &crate::services::task_scheduler::TaskSchedulers,
    ) -> Result<Self, String> {
        Ok(Self)
    }

    fn service_resources(&mut self, _world: &mut ipp_core::HostRuntime) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Default)]
struct ResourceProgressPlatform {
    service_calls: usize,
    progress_calls: usize,
}

impl HostServices for ResourceProgressPlatform {
    const NAME: &'static str = "resource-progress-test";

    fn initialize(
        _: &mut ipp_core::HostRuntime,
        _schedulers: &crate::services::task_scheduler::TaskSchedulers,
    ) -> Result<Self, String> {
        Ok(Self::default())
    }

    fn service_resources(&mut self, _: &mut ipp_core::HostRuntime) -> Result<(), String> {
        self.service_calls += 1;
        Ok(())
    }

    fn progress_resources(&mut self, _: &mut ipp_core::HostRuntime) {
        self.progress_calls += 1;
    }
}

#[test]
fn resource_progress_does_not_admit_commands_or_step_worlds() {
    let mut host = Host::<ResourceProgressPlatform>::new().unwrap();
    host.open_session(1, &[]).unwrap();
    host.session_mut(1)
        .unwrap()
        .receive(&ipp_protocol::HELLO)
        .unwrap();
    host.session_mut(1).unwrap().take_response().unwrap();
    host.session_mut(1)
        .unwrap()
        .receive(&batch(1, 1, 1))
        .unwrap();

    for _ in 0..8 {
        host.progress_resources().unwrap();
    }
    host.service_resources().unwrap();

    let mut session = host.session_mut(1).unwrap();
    assert_eq!(session.world().tick(), 0);
    assert!(session.world().entities().is_empty());
    assert!(session.take_response().is_none());
    drop(session);
    assert_eq!(host.services.service_calls, 17);
    assert_eq!(host.services.progress_calls, 8);
}

fn ready(id: u64) -> Host<TestPlatform> {
    let mut session = Host::new().unwrap();
    session.open_session(id, &[]).unwrap();
    assert!(!session.test_session().is_ready());
    session
        .test_session()
        .receive(&ipp_protocol::HELLO)
        .unwrap();
    assert!(session.test_session().is_ready());
    session.test_session().take_response().unwrap();
    session
}

fn request(session: u64, request_id: u64, tag: u8) -> Vec<u8> {
    let mut bytes = session.to_le_bytes().to_vec();
    bytes.extend_from_slice(&request_id.to_le_bytes());
    bytes.push(tag);
    if tag == 3 {
        bytes.push(1);
        bytes.extend_from_slice(&[0; 16]);
        bytes.extend_from_slice(&256u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
    }
    bytes
}

fn batch(session: u64, request_id: u64, alias: u32) -> Vec<u8> {
    let mut bytes = request(session, request_id, 1);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&alias.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    bytes
}

#[test]
fn receive_is_queue_only_and_repeated_batch_ids_keep_request_correlation() {
    let mut session = ready(1);
    session.test_session().receive(&batch(1, 11, 1)).unwrap();
    session.test_session().receive(&batch(1, 12, 2)).unwrap();
    assert_eq!(session.test_session().world().tick(), 0);
    assert!(session.test_session().world().entities().is_empty());
    assert!(session.test_session().take_response().is_none());

    session.tick(0.0).unwrap();
    assert_eq!(session.test_session().world().entities().len(), 2);
    let first = session.test_session().take_response().unwrap();
    let second = session.test_session().take_response().unwrap();
    assert_eq!(&first[8..16], &11u64.to_le_bytes());
    assert_eq!(&second[8..16], &12u64.to_le_bytes());
    assert_eq!(first[24], 1);
    assert_eq!(second[24], 1);
    assert_eq!(session.test_session().take_response().unwrap()[24], 4);
}

#[test]
fn admitted_requests_stay_counted_until_their_replies_complete() {
    let mut session = ready(1);
    for request_id in 1..=MAX_PENDING as u64 {
        session
            .test_session()
            .receive(&request(1, request_id, 3))
            .unwrap();
    }
    assert!(session.test_session().receive(&request(1, 65, 3)).is_err());
    session.tick(0.0).unwrap();
    assert_eq!(session.test_session().world().tick(), 1);
    assert!(session.test_session().receive(&request(1, 66, 3)).is_err());
    assert_eq!(session.test_session().session.outbox.len(), MAX_PENDING);
    assert_eq!(session.sessions[&1].progress.unwrap().tick, 1);

    let replies: Vec<_> = std::iter::from_fn(|| session.test_session().take_response())
        .take(MAX_PENDING)
        .collect();
    assert!(session.test_session().receive(&request(1, 67, 3)).is_err());
    drop(replies);
    session.test_session().receive(&request(1, 68, 3)).unwrap();
}

#[test]
fn invalid_time_never_consumes_pending_work_or_advances_the_world() {
    for dt in [f64::NAN, f64::INFINITY, -0.01] {
        let mut session = ready(1);
        session.test_session().receive(&batch(1, 1, 1)).unwrap();
        assert!(session.tick(dt).is_err());
        assert_eq!(session.test_session().session.pending.len(), 1);
        assert_eq!(session.test_session().world().tick(), 0);
        assert!(session.test_session().world().entities().is_empty());
        session.tick(0.0).unwrap();
        assert_eq!(session.test_session().world().entities().len(), 1);
    }
}

#[test]
fn bootstrap_and_request_sessions_are_isolated() {
    let mut session = Host::<TestPlatform>::new().unwrap();
    session.open_session(2, &[]).unwrap();
    session
        .test_session()
        .receive(&ipp_protocol::HELLO)
        .unwrap();
    session.test_session().take_response().unwrap();
    assert!(session.test_session().receive(&request(1, 1, 3)).is_err());
    assert_eq!(session.test_session().world().tick(), 0);
    assert!(session.test_session().session.pending.is_empty());
}

struct PresentationPlatform {
    failing: Option<ipp_core::WorldId>,
    input_failure: Option<ipp_core::OutputRef>,
}

impl HostServices for PresentationPlatform {
    const NAME: &'static str = "presentation-test";

    fn initialize(
        _: &mut ipp_core::HostRuntime,
        _schedulers: &crate::services::task_scheduler::TaskSchedulers,
    ) -> Result<Self, String> {
        Ok(Self {
            failing: None,
            input_failure: None,
        })
    }

    fn service_resources(&mut self, _: &mut ipp_core::HostRuntime) -> Result<(), String> {
        Ok(())
    }

    fn route_input(
        &mut self,
        _: &mut ipp_core::HostRuntime,
        _: &ipp_core::HostFrameReport,
    ) -> Vec<HostInputFailure> {
        self.input_failure
            .map(|output| HostInputFailure {
                output,
                message: "failed test input context".into(),
            })
            .into_iter()
            .collect()
    }

    fn present(
        &mut self,
        _: &ipp_core::HostRuntime,
        output: ipp_core::OutputRef,
        publication: ipp_core::WorldPublicationId,
        _: ipp_core::WorldViewport,
        _: f64,
        completion: crate::PresentationCompletion<'_>,
    ) -> Result<PresentationDrawSummary, HostPresentationFailure> {
        let crate::PresentationCompletion {
            capture: _,
            outputs,
        } = completion;
        if self.failing == Some(output.world().id()) {
            return Err(HostPresentationFailure {
                scope: ipp_protocol::RuntimeFailureScope::Context,
                message: "lost test context".into(),
            });
        }
        for observed in outputs {
            if observed.output == output {
                observed.publication = Some(publication);
            }
        }
        Ok(PresentationDrawSummary::default())
    }

    fn presentation_surface(
        &self,
    ) -> Result<
        ipp_protocol::presentation::PresentationSurface,
        ipp_protocol::presentation::PresentationError,
    > {
        Ok(ipp_protocol::presentation::PresentationSurface {
            id: 1,
            context: 1,
            max_width: 4096,
            max_height: 4096,
        })
    }

    fn configure_presentation(
        &mut self,
        _: ipp_core::WorldViewport,
    ) -> Result<(), ipp_protocol::presentation::PresentationError> {
        Ok(())
    }
}

#[test]
fn recoverable_presentation_failure_publishes_commits_once_and_keeps_peer_world_running() {
    scoped_platform_failure_preserves_outcomes(false);
}

#[test]
fn postcommit_input_failure_publishes_commits_once_and_keeps_peer_world_running() {
    scoped_platform_failure_preserves_outcomes(true);
}

fn scoped_platform_failure_preserves_outcomes(input_failure: bool) {
    let mut host = Host::<PresentationPlatform>::new().unwrap();
    for id in [1, 2] {
        host.open_session(id, crate::host::TEST_CAMERA_SYSTEMS)
            .unwrap();
        let mut session = host.session_mut(id).unwrap();
        session.receive(&ipp_protocol::HELLO).unwrap();
        session.take_response().unwrap();
        session.receive(&batch(id, 11, 1)).unwrap();
    }
    let world = host.session_world(1).unwrap();
    host.runtime_mut()
        .world_mut(world)
        .unwrap()
        .enqueue(ipp_core::Batch {
            id: 99,
            operations: vec![
                ipp_core::Command::Create {
                    alias: 0,
                    metadata: Default::default(),
                    adopt: false,
                },
                ipp_core::Command::insert_value(
                    ipp_core::EntityRef::Alias(0),
                    ipp_core::ComponentValue::Camera(Default::default()),
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

    let reference = host.runtime().world_ref(world).unwrap();
    let output = host
        .runtime_mut()
        .bind_output(reference, entity, ipp_core::OutputKind::Camera)
        .unwrap();
    host.runtime_mut()
        .set_root_output(
            output,
            ipp_core::WorldViewport {
                width: 640,
                height: 480,
                device_pixel_ratio: 1.0,
            },
        )
        .unwrap();
    let binding = host
        .runtime
        .root_output_binding(reference)
        .unwrap()
        .unwrap()
        .into();
    let surface = host.services.presentation_surface().unwrap();
    host.presentation.request(
        &mut host.runtime,
        &mut host.services,
        7,
        1,
        ipp_protocol::presentation::PresentationRequest::Select {
            surface,
            binding,
        },
        std::time::Duration::ZERO,
    );
    if input_failure {
        host.services_mut().input_failure = Some(output);
    } else {
        host.services_mut().failing = Some(world);
    }
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    for id in [1, 2] {
        let mut session = host.session_mut(id).unwrap();
        assert_eq!(session.world().entities().len(), 1 + usize::from(id == 1));
        let mut tags = Vec::new();
        while let Some(bytes) = session.take_response() {
            if bytes[24] == 1 {
                assert_eq!(&bytes[8..16], &11u64.to_le_bytes());
            }
            tags.push(bytes[24]);
        }
        assert_eq!(tags.iter().filter(|&&tag| tag == 1).count(), 1);
        assert_eq!(
            tags.iter().filter(|&&tag| tag == 22).count(),
            usize::from(id == 1)
        );
        assert!(tags.contains(&4));
    }
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    while let Some(bytes) = host.session_mut(1).unwrap().take_response() {
        assert_ne!(
            bytes[24], 22,
            "unchanged context failure is not repeated each frame"
        );
    }
    host.services_mut().failing = None;
    host.services_mut().input_failure = None;
    host.session_mut(1)
        .unwrap()
        .receive(&batch(1, 12, 2))
        .unwrap();
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    assert_eq!(host.session_mut(1).unwrap().world().entities().len(), 3);
}

#[test]
fn retained_responses_preserve_credit_exclusivity_and_session_fences() {
    let mut host = ready(1);
    let account = host.sessions[&1].reply_budget.0.clone();
    assert_eq!(account.usage().entries, 0);
    host.tick(0.0).unwrap();
    let first = host.test_session().take_response().unwrap();
    let original = first.to_vec();
    let address = first.as_ptr();
    let first_charge = account.usage();
    assert_eq!(first_charge.entries, 1);
    assert!(first_charge.bytes >= first.len());
    host.tick(0.0).unwrap();
    host.test_session()
        .queue_response(
            0,
            ResponseBody::RuntimeFailure {
                scope: ipp_protocol::RuntimeFailureScope::Resource,
                faulted: false,
                message: "retained semantic response".into(),
            },
        )
        .unwrap();
    let second = host.test_session().take_response().unwrap();
    assert_ne!(second.as_ptr(), address);
    assert_eq!(&first[..], original);
    assert_eq!(account.usage().entries, 2);
    drop(first);
    assert_eq!(account.usage().entries, first_charge.entries);
    host.close_session(1);
    assert_eq!(
        account.status(),
        ipp_core::services::reliable_output::OutputStatus::Closed
    );
    assert_eq!(account.usage().entries, 1);
    host.open_session(2, &[]).unwrap();
    host.session_mut(2)
        .unwrap()
        .receive(&ipp_protocol::HELLO)
        .unwrap();
    drop(host.session_mut(2).unwrap().take_response().unwrap());
    host.tick(0.0).unwrap();
    let bytes = host.session_mut(2).unwrap().take_response().unwrap();
    assert_eq!(&bytes[..8], &2u64.to_le_bytes());
    assert_eq!(&bytes[8..16], &0u64.to_le_bytes());
    assert_eq!(bytes[24], 4);
    assert_eq!(bytes.len(), 33);
    assert_eq!(account.usage().entries, 1);
    drop(second);
    assert_eq!(account.usage(), Default::default());
}

#[test]
fn dequeued_inflight_frames_cannot_escape_connection_capacity() {
    let mut host = ready(1);
    let account = host.sessions[&1].reply_budget.0.clone();
    host.tick(0.0).unwrap();
    let inflight = host.test_session().take_response().unwrap();
    for _ in 0..2 * MAX_PENDING {
        host.tick(0.0).unwrap();
        assert!(host.test_session().take_response().is_none());
    }
    assert_eq!(account.usage().entries, 1);
    drop(inflight);
    assert_eq!(account.usage(), Default::default());
    assert_eq!(host.test_session().take_response().unwrap()[24], 4);
}
