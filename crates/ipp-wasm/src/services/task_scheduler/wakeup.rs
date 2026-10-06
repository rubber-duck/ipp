//! Enqueue-only browser event-loop wakeup; never borrows the Host or a World.

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "ipp_tasks")]
unsafe extern "C" {
    #[link_name = "request_update"]
    fn request_update_import();
}

pub(crate) fn request_update() {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: The worker import only enqueues a later service turn. It accepts
        // no pointers, retains no memory and cannot reenter the borrowed Host.
        unsafe { request_update_import() };
    }
}
