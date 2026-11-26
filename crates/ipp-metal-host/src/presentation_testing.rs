//! Instrumentation-only control for exercising recovery through the native Host.

use std::sync::OnceLock;

static RECOVER_AFTER_FRAME: OnceLock<u64> = OnceLock::new();

pub(super) fn configure(frame: u64) -> Result<(), &'static str> {
    if frame == 0 {
        return Err("recovery test frame must be positive");
    }
    RECOVER_AFTER_FRAME
        .set(frame)
        .map_err(|_| "recovery test already configured")
}

pub(super) fn recovery_frame() -> Option<u64> {
    RECOVER_AFTER_FRAME.get().copied()
}
