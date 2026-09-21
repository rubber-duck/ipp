use super::*;

struct TestPlatform;

impl HostServices for TestPlatform {
    const NAME: &'static str = "test";

    fn initialize(_world: &mut ipp_core::HostRuntime) -> Result<Self, String> {
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

    fn initialize(_: &mut ipp_core::HostRuntime) -> Result<Self, String> {
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
    host.open_session(1).unwrap();
    host.session_mut(1)
        .unwrap()
        .receive(&ipp_protocol::bootstrap())
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
    session.open_session(id).unwrap();
    assert!(!session.test_session().is_ready());
    session
        .test_session()
        .receive(&ipp_protocol::bootstrap())
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
    }
    bytes
}

fn batch(session: u64, request_id: u64, alias: u32) -> Vec<u8> {
    let mut bytes = request(session, request_id, 1);
    bytes.extend_from_slice(&7u64.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&alias.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&0u32.to_le_bytes());
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
fn ingress_and_output_share_one_bounded_budget() {
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
    assert_eq!(session.test_session().session.outbox.len(), MAX_PENDING + 1);
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
    session.open_session(2).unwrap();
    session
        .test_session()
        .receive(&ipp_protocol::bootstrap())
        .unwrap();
    session.test_session().take_response().unwrap();
    assert!(session.test_session().receive(&request(1, 1, 3)).is_err());
    assert_eq!(session.test_session().world().tick(), 0);
    assert!(session.test_session().session.pending.is_empty());
}

struct PresentationPlatform {
    failing: Option<ipp_core::WorldId>,
}

impl HostServices for PresentationPlatform {
    const NAME: &'static str = "presentation-test";

    fn initialize(_: &mut ipp_core::HostRuntime) -> Result<Self, String> {
        Ok(Self {
            failing: None,
        })
    }

    fn service_resources(&mut self, _: &mut ipp_core::HostRuntime) -> Result<(), String> {
        Ok(())
    }

    fn present(&mut self, world: &mut WorldContext<'_>) -> Result<(), HostPresentationFailure> {
        if self.failing == Some(world.id()) {
            return Err(HostPresentationFailure {
                scope: ipp_protocol::RuntimeFailureScope::Context,
                message: "lost test context".into(),
            });
        }
        Ok(())
    }
}

#[test]
fn recoverable_presentation_failure_publishes_commits_once_and_keeps_peer_world_running() {
    let mut host = Host::<PresentationPlatform>::new().unwrap();
    for id in [1, 2] {
        host.open_session(id).unwrap();
        let mut session = host.session_mut(id).unwrap();
        session.receive(&ipp_protocol::bootstrap()).unwrap();
        session.take_response().unwrap();
        session.receive(&batch(id, 11, 1)).unwrap();
    }
    host.services_mut().failing = host.session_world(1);
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    for id in [1, 2] {
        let mut session = host.session_mut(id).unwrap();
        assert_eq!(session.world().entities().len(), 1);
        let mut tags = Vec::new();
        while let Some(bytes) = session.take_response() {
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
    host.session_mut(1)
        .unwrap()
        .receive(&batch(1, 12, 2))
        .unwrap();
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    assert_eq!(host.session_mut(1).unwrap().world().entities().len(), 2);
}

#[test]
fn response_buffers_preserve_exclusivity_and_session_fences_with_optional_reuse() {
    let reuse = ipp_core::allocation_followup_enabled();
    let mut host = ready(1);
    host.tick(0.0).unwrap();
    let first = host.test_session().take_response().unwrap();
    let original = first.clone();
    let address = first.as_ptr();
    host.tick(0.0).unwrap();
    let second = host.test_session().take_response().unwrap();
    assert_ne!(second.as_ptr(), address);
    assert_eq!(first, original);
    host.recycle_response_buffer(first);
    if !reuse {
        assert!(host.response_buffers.is_empty());
    }
    host.tick(0.0).unwrap();
    let third = host.test_session().take_response().unwrap();
    if reuse {
        assert_eq!(third.as_ptr(), address);
    }
    assert_eq!(&third[..8], &1u64.to_le_bytes());
    assert_eq!(&third[8..16], &0u64.to_le_bytes());
    assert_eq!(third[24], 4); // Complete unsolicited frame.
    assert_eq!(third.len(), 33);
    assert_ne!(third, original);
    host.close_session(1);
    host.recycle_response_buffer(second);
    host.open_session(2).unwrap();
    host.session_mut(2)
        .unwrap()
        .receive(&ipp_protocol::bootstrap())
        .unwrap();
    host.session_mut(2).unwrap().take_response().unwrap();
    host.tick(0.0).unwrap();
    let bytes = host.session_mut(2).unwrap().take_response().unwrap();
    assert_eq!(&bytes[..8], &2u64.to_le_bytes());
    assert_eq!(&bytes[8..16], &0u64.to_le_bytes());
    assert_eq!(bytes[24], 4);
    assert_eq!(bytes.len(), 33);
}

#[test]
fn returned_external_buffers_do_not_grow_the_free_pool_without_bound() {
    let reuse = ipp_core::allocation_followup_enabled();
    let mut host = ready(1);
    let capacity = host.response_buffers.capacity();
    for _ in 0..capacity * 3 {
        let mut bytes = Vec::with_capacity(64);
        bytes.extend_from_slice(b"previous response");
        host.recycle_response_buffer(bytes);
    }
    if reuse {
        assert_eq!(host.response_buffers.len(), capacity);
        assert!(host.response_buffers.iter().all(Vec::is_empty));
    } else {
        assert!(host.response_buffers.is_empty());
    }
    assert_eq!(host.response_buffers.capacity(), capacity);
    host.open_session(2).unwrap();
    host.session_mut(2).unwrap();
    if reuse {
        assert!(host.response_buffers.capacity() >= 4 * MAX_OUTBOX);
    } else {
        assert_eq!(host.response_buffers.capacity(), capacity);
    }
}
