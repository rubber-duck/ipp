use super::*;
use crate::HostServices;
use ipp_protocol::host::profiling::*;

struct Headless;

impl HostServices for Headless {
    const NAME: &'static str = "profile-test";

    fn service_resources(&mut self, _: &mut ipp_core::HostRuntime) -> Result<(), String> {
        Ok(())
    }

    fn initialize(
        _: &mut ipp_core::HostRuntime,
        _schedulers: &crate::services::task_scheduler::TaskSchedulers,
    ) -> Result<Self, String> {
        Ok(Self)
    }
}

#[test]
fn owner_cancellation_capacity_and_disconnection_leave_capture_reusable() {
    let mut profile = HostProfiling::default();
    let mut services = Headless;
    let start = |limit| ProfileRequest::Start {
        counters: true,
        max_artifact_bytes: limit,
        gpu: ProfileGpuSampling::Off,
        gl_calls: false,
        max_events: 0,
    };
    let first = profile.request(99001, Headless::NAME, 1, start(1), &mut services);
    assert_eq!(first.status, ProfileStatus::Available);
    assert_eq!(
        profile
            .request(99001, Headless::NAME, 2, start(4096), &mut services)
            .status,
        ProfileStatus::Busy
    );
    assert_eq!(
        profile
            .request(
                99001,
                Headless::NAME,
                2,
                ProfileRequest::Release(first.capture),
                &mut services
            )
            .status,
        ProfileStatus::InvalidCapture
    );
    assert_eq!(
        profile
            .request(
                99001,
                Headless::NAME,
                1,
                ProfileRequest::Stop(first.capture),
                &mut services
            )
            .status,
        ProfileStatus::Capacity
    );
    assert_eq!(
        profile
            .request(
                99001,
                Headless::NAME,
                1,
                ProfileRequest::Release(first.capture),
                &mut services
            )
            .status,
        ProfileStatus::Available
    );
    let second = profile.request(99001, Headless::NAME, 1, start(65536), &mut services);
    assert_ne!(first.capture, second.capture);
    assert_eq!(
        profile
            .request(
                99001,
                Headless::NAME,
                1,
                ProfileRequest::Release(second.capture),
                &mut services
            )
            .status,
        ProfileStatus::Available
    );
    let third = profile.request(99001, Headless::NAME, 1, start(65536), &mut services);
    profile.disconnect(1);
    profile.cleanup(&mut services);
    let fourth = profile.request(99001, Headless::NAME, 2, start(65536), &mut services);
    assert_eq!(fourth.status, ProfileStatus::Available);
    assert_ne!(third.capture, fourth.capture);
    assert_eq!(
        profile
            .request(
                99001,
                Headless::NAME,
                2,
                ProfileRequest::Release(fourth.capture),
                &mut services
            )
            .status,
        ProfileStatus::Available
    );
}
