//! Framebuffer bindings, viewport and depth writes as last set by this device.
//!
//! Offscreen passes (Surface cache repaints, glyph atlas population, shadow
//! maps and the linear frame target) save and restore the target they
//! interrupt. Values the device set are known exactly, so saving needs no
//! synchronous state query. The Host regains the context after each frame end,
//! so the device then forgets every value and queries an unknown one once when
//! a pass next needs it.

use super::GlesRenderDevice;
use std::cell::Cell;

pub(super) const FRAMEBUFFER: u32 = 0x8D40;
pub(super) const READ_FRAMEBUFFER: u32 = 0x8CA8;
pub(super) const DRAW_FRAMEBUFFER: u32 = 0x8CA9;
const DRAW_FRAMEBUFFER_BINDING: u32 = 0x8CA6;
const READ_FRAMEBUFFER_BINDING: u32 = 0x8CAA;
const VIEWPORT: u32 = 0x0BA2;
const DEPTH_WRITEMASK: u32 = 0x0B72;

/// Known context values; `None` means the Host may have changed it.
#[derive(Default)]
pub(super) struct GlesTargetState {
    draw: Cell<Option<u32>>,
    read: Cell<Option<u32>>,
    viewport: Cell<Option<[i32; 4]>>,
    depth_mask: Cell<Option<bool>>,
}

impl GlesTargetState {
    /// Forget every value when the Host may change them.
    pub(super) fn forget(&self) {
        self.draw.set(None);
        self.read.set(None);
        self.viewport.set(None);
        self.depth_mask.set(None);
    }
}

/// Draw and read framebuffers, viewport and depth writes to restore after a pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GlesTarget {
    pub(super) draw: u32,
    pub(super) read: u32,
    pub(super) viewport: [i32; 4],
    pub(super) depth_mask: bool,
}

impl GlesRenderDevice {
    /// The current target, querying only values the device has not set since
    /// the Host last had the context.
    pub(super) fn current_target(&self) -> GlesTarget {
        let state = &self.targets;
        let integer = |name| {
            let mut value = 0;
            // SAFETY: The current context writes one integer into this exclusive
            // local; GL retains no pointer.
            unsafe { (self.gl.get_integer)(name, &mut value) };
            value
        };
        let draw = state
            .draw
            .get()
            .unwrap_or_else(|| integer(DRAW_FRAMEBUFFER_BINDING) as u32);
        let read = state
            .read
            .get()
            .unwrap_or_else(|| integer(READ_FRAMEBUFFER_BINDING) as u32);
        let viewport = state.viewport.get().unwrap_or_else(|| {
            let mut viewport = [0; 4];
            // SAFETY: VIEWPORT writes exactly four integers into this exclusive
            // local array in the current context; GL retains no pointer.
            unsafe { (self.gl.get_integer)(VIEWPORT, viewport.as_mut_ptr()) };
            viewport
        });
        let depth_mask = state
            .depth_mask
            .get()
            .unwrap_or_else(|| integer(DEPTH_WRITEMASK) != 0);
        let target = GlesTarget {
            draw,
            read,
            viewport,
            depth_mask,
        };
        state.draw.set(Some(draw));
        state.read.set(Some(read));
        state.viewport.set(Some(viewport));
        state.depth_mask.set(Some(depth_mask));
        target
    }

    /// Record that `framebuffer` is about to be deleted. Deleting a bound
    /// framebuffer rebinds zero, and GL may reuse the name for a new object.
    pub(super) fn forget_framebuffer(&self, framebuffer: u32) {
        for binding in [&self.targets.draw, &self.targets.read] {
            if binding.get() == Some(framebuffer) {
                binding.set(None);
            }
        }
    }

    /// Bind draw and read framebuffers, skipping bindings already current.
    pub(super) fn bind_framebuffers(&self, draw: u32, read: u32) {
        let state = &self.targets;
        let draw_changed = state.draw.get() != Some(draw);
        let read_changed = state.read.get() != Some(read);
        // SAFETY: The names are live framebuffers of this current context or
        // borrowed Host handles; binding copies scalars and retains no pointer.
        unsafe {
            if draw_changed && read_changed && draw == read {
                (self.gl.bind_framebuffer)(FRAMEBUFFER, draw);
            } else {
                if draw_changed {
                    (self.gl.bind_framebuffer)(DRAW_FRAMEBUFFER, draw);
                }
                if read_changed {
                    (self.gl.bind_framebuffer)(READ_FRAMEBUFFER, read);
                }
            }
        }
        state.draw.set(Some(draw));
        state.read.set(Some(read));
    }

    /// Bind only the draw framebuffer, leaving the read binding unchanged.
    #[cfg(feature = "shadows")]
    pub(super) fn bind_draw_framebuffer(&self, draw: u32) {
        if self.targets.draw.replace(Some(draw)) != Some(draw) {
            // SAFETY: A live framebuffer of this current context or a borrowed
            // Host handle; binding copies a scalar and retains no pointer.
            unsafe { (self.gl.bind_framebuffer)(DRAW_FRAMEBUFFER, draw) };
        }
    }

    pub(super) fn set_viewport(&self, viewport: [i32; 4]) {
        if self.targets.viewport.replace(Some(viewport)) != Some(viewport) {
            // SAFETY: Scalar context state in the current context only.
            unsafe { (self.gl.viewport)(viewport[0], viewport[1], viewport[2], viewport[3]) };
        }
    }

    pub(super) fn set_depth_mask(&self, enabled: bool) {
        if self.targets.depth_mask.replace(Some(enabled)) != Some(enabled) {
            // SAFETY: Scalar context state in the current context only.
            unsafe { (self.gl.depth_mask)(u8::from(enabled)) };
        }
    }

    /// Rebind a target saved by [`Self::current_target`].
    #[cfg(feature = "surfaces")]
    pub(super) fn restore_target(&self, target: GlesTarget) {
        self.bind_framebuffers(target.draw, target.read);
        self.set_viewport(target.viewport);
        self.set_depth_mask(target.depth_mask);
    }
}
