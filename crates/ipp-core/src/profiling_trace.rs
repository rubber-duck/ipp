//! Complete CPU spans in a preallocated, evaluation-thread-owned drop-new buffer.
//! This clock is independent of renderer draw ordinals and GPU clocks.
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

use crate::profiling::{ProfileContext, capture_id};

static ENABLED: AtomicBool = AtomicBool::new(false);

/// One completed CPU scope. Identities are copied when the scope starts.
#[derive(Clone, Copy, Debug)]
pub struct ProfileSpan {
    /// Issuance ordinal within this capture.
    pub sequence: u64,
    /// World, System or fixed scope.
    pub kind: &'static str,
    /// Prepared static scope name.
    pub name: &'static str,
    /// Semantic context copied at issuance.
    pub context: ProfileContext,
    /// Start in ipp-core.monotonic nanoseconds.
    pub start: u64,
    /// End in the same clock domain.
    pub end: u64,
    /// Capture-scoped Host frame ordinal, absent outside a frame.
    pub host_frame: Option<u64>,
    /// Capture-scoped evaluation thread ordinal.
    pub thread: u64,
}

#[derive(Default)]
struct Recorder {
    capture: u64,
    enabled: bool,
    capacity: usize,
    sequence: u64,
    dropped: u64,
    frame: u64,
    current_frame: Option<u64>,
    spans: Vec<ProfileSpan>,
}

thread_local! {
    static RECORDER: RefCell<Recorder> = const { RefCell::new(Recorder {
        capture: 0, enabled: false, capacity: 0, sequence: 0, dropped: 0,
        frame: 0, current_frame: None, spans: Vec::new(),
    }) };
}

/// Reserve bounded storage before enabling measurements. No event-time allocation.
pub fn prepare(capacity: usize) -> Result<(), std::collections::TryReserveError> {
    let mut spans = Vec::new();
    spans.try_reserve_exact(capacity)?;
    RECORDER.with(|state| {
        *state.borrow_mut() = Recorder {
            capacity,
            spans,
            ..Recorder::default()
        }
    });
    Ok(())
}

pub(crate) fn enable() {
    RECORDER.with(|state| {
        let mut state = state.borrow_mut();
        state.spans.clear();
        state.sequence = 0;
        state.dropped = 0;
        state.frame = 0;
        state.current_frame = None;
        state.capture = capture_id();
        state.enabled = state.capacity != 0;
        ENABLED.store(state.enabled, Relaxed);
    });
}

pub(crate) fn enabled() -> bool {
    if !ENABLED.load(Relaxed) {
        return false;
    }
    RECORDER
        .try_with(|state| {
            let state = state.borrow();
            state.enabled && state.capture == capture_id()
        })
        .unwrap_or(false)
}

pub(crate) fn pause() {
    ENABLED.store(false, Relaxed);
    RECORDER.with(|state| state.borrow_mut().enabled = false);
}

pub(crate) fn release() {
    ENABLED.store(false, Relaxed);
    RECORDER.with(|state| *state.borrow_mut() = Recorder::default());
}

pub(crate) fn begin(
    kind: &'static str,
    name: &'static str,
    context: ProfileContext,
    start: u64,
) -> ProfileSpan {
    RECORDER.with(|state| {
        let mut state = state.borrow_mut();
        state.sequence += 1;
        ProfileSpan {
            sequence: state.sequence,
            kind,
            name,
            context,
            start,
            end: start,
            host_frame: state.current_frame,
            thread: 1,
        }
    })
}

pub(crate) fn finish(mut span: ProfileSpan, end: u64) {
    RECORDER.with(|state| {
        let mut state = state.borrow_mut();
        if !state.enabled || state.capture != capture_id() {
            return;
        }
        if state.spans.len() == state.capacity {
            state.dropped = state.dropped.saturating_add(1);
            return;
        }
        span.end = end;
        state.spans.push(span);
    });
}

/// Visit complete spans ordered by issuance. Invoke only after pausing capture.
/// Returning false stops bounded serialization without another event allocation.
pub fn visit(mut visitor: impl FnMut(&ProfileSpan) -> bool) {
    RECORDER.with(|state| {
        let mut state = state.borrow_mut();
        assert!(!state.enabled, "trace readback requires paused capture");
        state.spans.sort_unstable_by_key(|span| span.sequence);
        for span in &state.spans {
            if !visitor(span) {
                break;
            }
        }
    });
}

/// Capacity, dropped complete spans and retained backing bytes.
pub fn retention() -> (usize, u64, usize) {
    RECORDER.with(|state| {
        let state = state.borrow();
        (
            state.capacity,
            state.dropped,
            state.spans.capacity() * std::mem::size_of::<ProfileSpan>(),
        )
    })
}

pub(crate) struct FrameScope {
    previous: Option<u64>,
    capture: u64,
}

pub(crate) fn frame() -> Option<FrameScope> {
    if !enabled() || !crate::profiling::measuring() {
        return None;
    }
    RECORDER.with(|state| {
        let mut state = state.borrow_mut();
        let previous = state.current_frame;
        state.frame += 1;
        state.current_frame = Some(state.frame);
        Some(FrameScope {
            previous,
            capture: capture_id(),
        })
    })
}

impl Drop for FrameScope {
    fn drop(&mut self) {
        if self.capture == capture_id() {
            RECORDER.with(|state| state.borrow_mut().current_frame = self.previous);
        }
    }
}
