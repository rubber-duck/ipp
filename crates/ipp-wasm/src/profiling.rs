//! Instrumentation-only trusted Host control and bounded immutable readback.

use ipp_protocol::profiling::{ProfileRequest, ProfileResponse, ProfileStatus};

#[global_allocator]
static ALLOCATOR: ipp_core::profiling::CountingAllocator = ipp_core::profiling::CountingAllocator;

std::thread_local! {
    static RESPONSE: std::cell::RefCell<ProfileResponse> = std::cell::RefCell::new(ProfileResponse::status(ProfileStatus::Unavailable));
}

/// Called outside frame guards; ownership uses the trusted worker adapter identity.
// SAFETY: Unique export accepts scalar controls and exposes no simulation stepping.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_profile_control(
    kind: u32,
    capture: u64,
    offset: u64,
    counters: u32,
    max_artifact_bytes: u64,
) -> u32 {
    let request = match kind {
        0 => ProfileRequest::Status,
        1 => ProfileRequest::Start {
            counters: counters != 0,
            max_artifact_bytes,
            gpu: ipp_protocol::profiling::ProfileGpuSampling::Off,
            gl_calls: false,
            max_events: 0,
        },
        2 => ProfileRequest::Stop(capture),
        3 => ProfileRequest::Read {
            capture,
            offset,
        },
        4 => ProfileRequest::Release(capture),
        _ => return ProfileStatus::InvalidCapture as u32,
    };
    let response = crate::BOUNDARY.with_borrow_mut(|boundary| boundary.profile_control(request));
    let status = response.status as u32;
    RESPONSE.with_borrow_mut(|slot| *slot = response);
    status
}

// SAFETY: Unique scalar metadata export, no retained pointers or mutable access.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_profile_response_capture() -> u64 {
    RESPONSE.with_borrow(|response| response.capture)
}

// SAFETY: Unique scalar metadata export, no retained pointers or mutable access.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_profile_response_total() -> u64 {
    RESPONSE.with_borrow(|response| response.total_bytes)
}

/// Read-only bytes remain valid until the next control call or Host disposal.
// SAFETY: The worker copies bytes synchronously before issuing the next control;
// it must refresh memory.buffer after a call that may grow linear memory.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_profile_response_ptr() -> *const u8 {
    RESPONSE.with_borrow(|response| response.bytes.as_ptr())
}

// SAFETY: Unique bounded response length export; no mutable access.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_profile_response_len() -> usize {
    RESPONSE.with_borrow(|response| response.bytes.len())
}
