//! Synchronous browser worker host with bounded, Rust-owned linear-memory I/O.
//!
//! One WASM instance belongs to one dedicated worker thread. Calls must not be
//! concurrent or reentrant. The host opens a strictly increasing nonzero session
//! ID, then reserves input, reacquires `memory.buffer`, writes only the reserved
//! bytes, and calls receive with exactly that length. The first message is the
//! production bootstrap; later messages use the negotiated binary contract.
//!
//! Input reservation pointers are valid only until the next open, close, reserve or
//! receive, tick, resource progress, poll, resource poll or resource completion. Output pointers are read-only until the next such call;
//! reacquire `memory.buffer` and copy output before making it. Accessors and the
//! schema hash do not invalidate buffers. Never retain JS views across mutating
//! exports: allocations can grow WASM memory. No caller pointer is accepted, no
//! Rust reference survives an export, and every allocation retains a Rust owner.
//!
//! Receive only queues ingress. The worker owns an autonomous clock and calls
//! `ipp_tick(dt)` once per frame, then drains `ipp_poll()` until it returns zero.
//! Drain after receive as well because bootstrap replies are immediately queued.
//! Invalid ingress or frame/output failure closes the session and exposes a
//! bounded UTF-8 diagnostic.
//! Core semantic rejections and oversized observations use protocol responses.
//! Closing drops the world and both buffers, retaining only an ID high-water mark
//! so delayed messages cannot target a later world under a recycled session ID.

#[cfg(feature = "diagnostics")]
macro_rules! diagnostic {
    ($($args:tt)*) => { ipp_core::diagnostic!($($args)*); };
}

#[cfg(not(feature = "diagnostics"))]
macro_rules! diagnostic {
    ($($args:tt)*) => {{}};
}

#[cfg(feature = "diagnostics")]
pub mod diagnostics;

use std::cell::RefCell;

mod boundary;
mod host;

mod services;

#[cfg(test)]
mod wasm_host_tests;

thread_local! {
    // wasm32-unknown-unknown is single-threaded; native tests get isolated owners.
    static BOUNDARY: RefCell<boundary::WasmHostBoundary> = const {
        RefCell::new(boundary::WasmHostBoundary::new())
    };
}

/// Start a fresh worker Host connection; IDs must strictly increase per instance.
/// Returns one on success, zero with a UTF-8 diagnostic on failure.
// SAFETY: Unique symbol; all state is owned and accessed on the calling worker.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_session_open(id: u64) -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| u32::from(boundary.open(id)))
}

/// Drop the world and I/O allocations. Previously issued pointers are invalid.
// SAFETY: Unique symbol; no host pointers are dereferenced or Rust borrows retained.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_session_close() {
    BOUNDARY.with_borrow_mut(boundary::WasmHostBoundary::close);
}

/// Reserve exactly 1..=1MiB zeroed bytes; returns null and closes on failure.
/// The host writes the reserved range only between this return and receive.
// SAFETY: Unique symbol; pointer refers to owned bytes, with no retained Rust alias.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_input_reserve(len: usize) -> *mut u8 {
    BOUNDARY.with_borrow_mut(|boundary| boundary.reserve(len))
}

/// Consume exactly the reserved bytes. One means accepted and queued;
/// zero means UTF-8 error output and a closed session. Reservations are single-use.
// SAFETY: Unique symbol; receive accesses only Rust-owned storage, never caller pointers.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_receive(len: usize) -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| u32::from(boundary.receive(len)))
}

/// Run one host-clock frame. One means success; zero means a closed session with
/// UTF-8 error output. This is a worker host operation, never a wire request.
// SAFETY: Unique symbol; all world access remains on the single owning worker.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_tick(dt: f64) -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| u32::from(boundary.tick(dt)))
}

/// Progress Host-owned providers and loaders without admitting commands,
/// evaluating or presenting a World, advancing its clock, or publishing events.
// SAFETY: Unique symbol; all service access remains on the single owning worker.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_progress_resources() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| u32::from(boundary.progress_resources()))
}

/// Expose Host-owned provider requests and cancellations without polling loaders.
// SAFETY: Unique symbol; all service access remains on the single owning worker.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_service_resources() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| u32::from(boundary.service_resources()))
}

/// Invalidate prior output and dequeue one response. One means output is ready;
/// zero means no output. Copy each response before calling poll again.
// SAFETY: Unique symbol; output retains a Rust owner and no Rust borrow escapes.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_poll() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| u32::from(boundary.poll()))
}

/// Host-only resource request/cancellation queue, separate from client messages.
/// Uses the same single-owner output buffer and invalidation rules as ipp_poll.
// SAFETY: Unique symbol; output is Rust-owned and no reference escapes the call.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_resource_poll() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| u32::from(boundary.resource_poll()))
}

/// Reserve owned provider bytes with fallible allocation, independently of
/// command framing. Shares the single-use input buffer and invalidation rules.
// SAFETY: Unique symbol; returns an owned buffer with no retained Rust alias.
// The pointer is invalidated by the next mutating export, including completion.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_resource_input_reserve(len: usize) -> *mut u8 {
    BOUNDARY.with_borrow_mut(|boundary| boundary.reserve_resource(len))
}

/// Consume reserved provider bytes (success=1) or UTF-8 error (success=0).
/// Only queues a completion; publication belongs to the next host-driven frame.
// SAFETY: Unique symbol; only owned reserved bytes are consumed, no caller pointer
// is accepted and no Rust borrow survives this single-worker call.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_resource_complete(session: u64, id: u64, success: u32, len: usize) -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        u32::from(boundary.resource_complete(session, id, success, len))
    })
}

/// Feed one bounded source chunk. Returns 1 accepted, 2 backpressure, 0 fatal.
// SAFETY: Unique symbol; scalar arguments index the exclusively owned input reservation.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_resource_chunk(session: u64, id: u64, len: usize) -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| boundary.asset_chunk(session, id, len))
}

/// Application-owned source pipes retained in Rust, excluding decoder/GPU storage.
// SAFETY: Unique symbol; scalar observation without borrowed storage escaping.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_resource_buffered_bytes() -> usize {
    BOUNDARY.with_borrow(|boundary| boundary.resource_buffered_bytes())
}

/// Shared producer and protocol UTF-8 error budget.
// SAFETY: Unique symbol; returns a constant without borrowing runtime state.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_asset_error_max_bytes() -> usize {
    ipp_core::services::asset_management::MAX_ASSET_ERROR_BYTES
}

/// End this source reader after all chunks, or report a bounded provider error.
// SAFETY: Unique symbol; exclusive owned input access, with no reference escaping.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_resource_end(session: u64, id: u64, success: u32, len: usize) -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| boundary.asset_end(session, id, success, len))
}

/// Read-only output pointer, or null when empty. Copy before the next mutating call.
// SAFETY: Unique symbol; returned pointer has the owned buffer lifetime described above.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_output_ptr() -> *const u8 {
    BOUNDARY.with_borrow(|boundary| boundary.output_ptr())
}

/// Exact output byte length, bounded to 1MiB.
// SAFETY: Unique symbol; returns a length without retaining a reference.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_output_len() -> usize {
    BOUNDARY.with_borrow(|boundary| boundary.output_len())
}

/// Compatibility identity of the actual runtime target and selected capabilities.
// SAFETY: Unique symbol; compatibility identity has no borrowed storage.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_schema_hash() -> u64 {
    ipp_protocol::schema_hash()
}

#[cfg(feature = "schema-export")]
mod contract {
    use std::sync::OnceLock;

    static CONTRACT: OnceLock<Vec<u8>> = OnceLock::new();
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();

    fn contract() -> &'static [u8] {
        CONTRACT.get_or_init(ipp_protocol::export_contract)
    }

    fn fixture() -> &'static [u8] {
        FIXTURE.get_or_init(ipp_protocol::export_layout_fixture)
    }

    // SAFETY: Unique exported symbol; pointer refers to immutable OnceLock-owned
    // bytes kept alive for the module lifetime. The host copies from linear memory.
    #[unsafe(no_mangle)]
    pub extern "C" fn ipp_contract_ptr() -> *const u8 {
        contract().as_ptr()
    }

    // SAFETY: Unique symbol; this reports the immutable buffer length only.
    #[unsafe(no_mangle)]
    pub extern "C" fn ipp_contract_len() -> usize {
        contract().len()
    }

    // SAFETY: Unique symbol; immutable OnceLock buffer remains alive and unaliased
    // by Rust mutation for the lifetime of the module.
    #[unsafe(no_mangle)]
    pub extern "C" fn ipp_fixture_ptr() -> *const u8 {
        fixture().as_ptr()
    }

    // SAFETY: Unique symbol; returns length without accessing external memory.
    #[unsafe(no_mangle)]
    pub extern "C" fn ipp_fixture_len() -> usize {
        fixture().len()
    }

    // SAFETY: Unique symbol; runs owned typed writes without host pointers.
    #[unsafe(no_mangle)]
    pub extern "C" fn ipp_fixture_check() -> u32 {
        u32::from(ipp_protocol::check_layout_fixture())
    }
}

/// Set browser-supplied identity entropy before the first World is created.
// SAFETY: Unique export; the integer is copied and all Host access remains worker-local.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_host_set_identity_namespace(namespace: u64) -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| u32::from(boundary.set_identity_namespace(namespace)))
}

/// Set the Host idle-resource retention target before the first World is created.
// SAFETY: Unique export; the integer is copied and all Host access remains worker-local.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_host_set_asset_cache_bytes(bytes: u32) -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| u32::from(boundary.set_asset_cache_bytes(bytes)))
}

/// Supply monotonic Host time, including while simulation is paused.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_host_time(seconds: f64) -> u32 {
    if !seconds.is_finite() || seconds < 0.0 {
        return 0;
    }
    let Ok(now) = std::time::Duration::try_from_secs_f64(seconds) else {
        return 0;
    };
    BOUNDARY.with_borrow_mut(|boundary| u32::from(boundary.maintain_connections(now)))
}

/// Host transport admission; callers retain bounded input while throttled.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_accepts_input() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| u32::from(boundary.accepts_input()))
}

#[cfg(feature = "profiling")]
mod profiling;
