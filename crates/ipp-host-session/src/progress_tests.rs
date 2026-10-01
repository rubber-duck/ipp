use super::*;
use ipp_core::services::reliable_output::OutputCharge;

struct Platform;

impl HostServices for Platform {
    const NAME: &'static str = "progress-test";

    fn initialize(_: &mut ipp_core::HostRuntime) -> Result<Self, String> {
        Ok(Self)
    }

    fn service_resources(&mut self, _: &mut ipp_core::HostRuntime) -> Result<(), String> {
        Ok(())
    }
}

fn ready() -> Host<Platform> {
    let mut host = Host::new().unwrap();
    host.open_session(1, &[]).unwrap();
    let mut session = host.session_mut(1).unwrap();
    session.receive(&ipp_protocol::HELLO).unwrap();
    drop(session.take_response().unwrap());
    drop(session);
    host
}

fn tick(response: &[u8]) -> u64 {
    u64::from_le_bytes(response[16..24].try_into().unwrap())
}

fn take(host: &mut Host<Platform>) -> Option<ReliableResponse> {
    host.sessions.get_mut(&1).unwrap().take_response()
}

#[test]
fn progress_waits_for_physical_completion_then_drains_the_latest_tick() {
    let mut host = ready();
    host.tick(0.1).unwrap();
    let (first, completion) = take(&mut host).unwrap().into_parts();
    let account = host.sessions[&1].reply_budget.0.clone();
    for _ in 0..200 {
        host.tick(0.1).unwrap();
        assert!(take(&mut host).is_none());
        assert_eq!(account.usage().entries, 1);
    }
    let latest = host.sessions[&1].progress.unwrap();
    assert!(latest.tick > tick(&first));
    drop(first);
    assert!(take(&mut host).is_none());
    drop(completion);
    let next = take(&mut host).unwrap();
    assert_eq!(tick(&next), latest.tick);
    assert_eq!(next[24], 4);
    assert!(take(&mut host).is_none());
    drop(next);
    assert_eq!(account.usage(), OutputCharge::default());
}

#[test]
fn copied_progress_keeps_its_delivery_fence_after_source_release_and_session_close() {
    let mut host = ready();
    host.tick(0.0).unwrap();
    let mut copy = take(&mut host).unwrap().prepare_copy().ok().unwrap();
    let account = host.sessions[&1].reply_budget.0.clone();
    let count = host.sessions[&1].progress_leases.clone();
    copy.release_source();
    for _ in 0..200 {
        host.tick(0.0).unwrap();
        assert!(take(&mut host).is_none());
    }
    assert_eq!(count.get(), 1);
    host.close_session(1);
    assert_eq!(count.get(), 1);
    assert_eq!(account.usage().entries, 1);
    drop(copy);
    assert_eq!(count.get(), 0);
    assert_eq!(account.usage(), OutputCharge::default());
}

#[test]
fn later_progress_never_overtakes_semantic_responses_or_blocks_their_delivery() {
    let mut host = ready();
    host.tick(0.1).unwrap();
    let first = take(&mut host).unwrap();
    let mut expected = Vec::new();
    for message in ["first effect", "second effect"] {
        host.tick(0.1).unwrap();
        let mut session = host.session_mut(1).unwrap();
        expected.push(session.world.tick());
        session
            .queue_response(
                0,
                ResponseBody::RuntimeFailure {
                    scope: ipp_protocol::RuntimeFailureScope::Resource,
                    faulted: false,
                    message: message.into(),
                },
            )
            .unwrap();
    }
    for (expected_tick, message) in expected.iter().zip(["first effect", "second effect"]) {
        let response = take(&mut host).unwrap();
        assert_eq!(tick(&response), *expected_tick);
        assert!(
            response
                .windows(message.len())
                .any(|bytes| bytes == message.as_bytes())
        );
    }
    assert!(take(&mut host).is_none());
    drop(first);
    let progress = take(&mut host).unwrap();
    assert_eq!(tick(&progress), expected[1]);
    assert_eq!(progress[24], 4);
}

#[test]
fn deferred_progress_waits_for_ordinary_capacity_and_retries_without_evaluation() {
    let mut host = ready();
    let account = host.sessions[&1].reply_budget.0.clone();
    let held = account
        .reserve(OutputCharge {
            entries: 0,
            bytes: crate::reliable_output::ORDINARY_OUTPUT_BYTES - account.usage().bytes,
        })
        .unwrap();
    for _ in 0..200 {
        host.tick(0.0).unwrap();
        assert!(take(&mut host).is_none());
    }
    let latest = host.sessions[&1].progress.unwrap().tick;
    drop(held);
    assert_eq!(tick(&take(&mut host).unwrap()), latest);
    assert_eq!(account.usage(), OutputCharge::default());
}

#[test]
fn progress_older_than_a_later_failure_response_is_not_reintroduced() {
    let mut host = ready();
    host.tick(0.0).unwrap();
    let latest = host.sessions[&1].progress.unwrap().tick;
    host.sessions[&1].outbox.observe_tick(latest + 1);
    assert!(take(&mut host).is_none());
    assert!(host.sessions[&1].progress.is_none());
}
