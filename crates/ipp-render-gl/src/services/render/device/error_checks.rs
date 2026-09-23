//! When the GL devices poll the synchronous context error state.
//!
//! `getError` waits for the GL implementation to process every queued command,
//! so each poll is a CPU/GPU synchronization. Allocations, compilation, resource
//! uploads and passes whose results are kept (Surface cache repaints, glyph
//! atlas population and shadow passes) check when they finish. Draws, uniforms,
//! transient streams, retained-batch and instance-stream replacement and the
//! frame start check only in exhaustive mode, which attributes an error to the
//! call that raised it.
//!
//! The frame end checks in exhaustive mode, after a frame that replaced retained
//! storage and otherwise on every [`FRAME_CHECK_INTERVAL`]th frame, starting with
//! the first. Other errors may therefore be reported at a later frame end.
//! Unchecked frames still detect context loss within the frame through the
//! backend's dedicated loss query.

/// Frames between sampled frame-end error checks outside exhaustive mode.
pub(super) const FRAME_CHECK_INTERVAL: u32 = 30;

/// Error-check schedule for one device and context.
#[derive(Debug)]
pub(super) struct RenderDeviceErrorChecks {
    exhaustive: bool,
    interval: u32,
    frames_until_check: u32,
    retained_upload: bool,
}

impl Default for RenderDeviceErrorChecks {
    fn default() -> Self {
        Self::sampled(FRAME_CHECK_INTERVAL)
    }
}

impl RenderDeviceErrorChecks {
    /// Check the first frame end and then every `interval`th; one checks every frame.
    pub(super) fn sampled(interval: u32) -> Self {
        Self {
            exhaustive: false,
            interval: interval.max(1),
            frames_until_check: 0,
            retained_upload: false,
        }
    }

    pub(super) fn set_exhaustive(&mut self, enabled: bool) {
        self.exhaustive = enabled;
    }

    /// Whether routine draw, uniform and stream operations check individually.
    /// The WebGL bridge keeps its own copy of this setting.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn exhaustive(&self) -> bool {
        self.exhaustive
    }

    /// Record a retained-storage replacement whose own check was deferred.
    #[cfg(any(feature = "surfaces", test))]
    pub(super) fn note_retained_upload(&mut self) {
        self.retained_upload = true;
    }

    /// Advance to the next frame and report whether this frame end checks.
    pub(super) fn frame_end_checks(&mut self) -> bool {
        let due = self.exhaustive || self.retained_upload || self.frames_until_check == 0;
        self.retained_upload = false;
        self.frames_until_check = if due {
            self.interval - 1
        } else {
            self.frames_until_check - 1
        };
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule(checks: &mut RenderDeviceErrorChecks, frames: usize) -> Vec<bool> {
        (0..frames).map(|_| checks.frame_end_checks()).collect()
    }

    #[test]
    fn sampled_frames_check_the_first_frame_and_every_interval() {
        let mut checks = RenderDeviceErrorChecks::sampled(3);
        assert_eq!(
            schedule(&mut checks, 7),
            [true, false, false, true, false, false, true]
        );
    }

    #[test]
    fn default_interval_bounds_delayed_attribution() {
        let mut checks = RenderDeviceErrorChecks::default();
        let checked = schedule(&mut checks, FRAME_CHECK_INTERVAL as usize * 2 + 1);
        let positions: Vec<_> = checked
            .iter()
            .enumerate()
            .filter_map(|(frame, checked)| checked.then_some(frame))
            .collect();
        let interval = FRAME_CHECK_INTERVAL as usize;
        assert_eq!(positions, [0, interval, interval * 2]);
    }

    #[test]
    fn interval_one_checks_every_frame() {
        let mut checks = RenderDeviceErrorChecks::sampled(1);
        assert_eq!(schedule(&mut checks, 4), [true; 4]);
        let mut clamped = RenderDeviceErrorChecks::sampled(0);
        assert_eq!(schedule(&mut clamped, 3), [true; 3]);
    }

    #[test]
    fn retained_uploads_force_that_frame_end_and_restart_sampling() {
        let mut checks = RenderDeviceErrorChecks::sampled(4);
        assert!(checks.frame_end_checks());
        assert!(!checks.frame_end_checks());
        checks.note_retained_upload();
        assert!(checks.frame_end_checks());
        assert_eq!(schedule(&mut checks, 4), [false, false, false, true]);
    }

    #[test]
    fn exhaustive_mode_checks_every_call_and_frame() {
        let mut checks = RenderDeviceErrorChecks::sampled(8);
        assert!(!checks.exhaustive());
        checks.set_exhaustive(true);
        assert!(checks.exhaustive());
        assert_eq!(schedule(&mut checks, 3), [true; 3]);
        checks.set_exhaustive(false);
        assert_eq!(
            schedule(&mut checks, 8),
            [false, false, false, false, false, false, false, true]
        );
    }
}
