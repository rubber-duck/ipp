//! Platform event-loop delay for Host-local service operations.

use std::{future::Future, pin::Pin, time::Duration};

/// Native reactor timer; readiness wakes the owning Host task without stepping a World.
#[cfg(not(target_arch = "wasm32"))]
pub fn native_delay(duration: Duration) -> Pin<Box<dyn Future<Output = ()> + 'static>> {
    Box::pin(async move {
        async_io::Timer::after(duration).await;
    })
}
