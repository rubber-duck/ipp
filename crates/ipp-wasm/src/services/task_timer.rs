//! Worker timers only wake owned Host tasks; callbacks never access Host/World state.

use std::{
    cell::RefCell,
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
    time::Duration,
};

#[link(wasm_import_module = "ipp_tasks")]
unsafe extern "C" {
    fn timer_start(id: u64, milliseconds: u32);

    fn timer_cancel(id: u64);
}

struct TimerWaiter {
    ready: bool,
    waker: Option<Waker>,
}

#[derive(Default)]
struct Timers {
    next: u64,
    waiters: BTreeMap<u64, TimerWaiter>,
}

thread_local! {
    static TIMERS: RefCell<Timers> = RefCell::new(Timers::default());
}

pub(crate) struct BrowserTaskTimer {
    milliseconds: u32,
    id: Option<u64>,
}

impl BrowserTaskTimer {
    pub(crate) fn new(duration: Duration) -> Self {
        Self {
            milliseconds: duration.as_millis().min(u128::from(u32::MAX)) as u32,
            id: None,
        }
    }
}

impl Future for BrowserTaskTimer {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let Some(id) = self.id else {
            let id = TIMERS.with(|timers| {
                let mut timers = timers.borrow_mut();
                let id = timers
                    .next
                    .checked_add(1)
                    .expect("Host timer identity exhausted");
                timers.next = id;
                timers.waiters.insert(
                    id,
                    TimerWaiter {
                        ready: false,
                        waker: Some(cx.waker().clone()),
                    },
                );
                id
            });
            self.id = Some(id);
            // SAFETY: Scalar import enqueues a later worker timer callback and retains
            // no Rust pointers. Registration precedes the import; callbacks never reenter.
            unsafe {
                timer_start(id, self.milliseconds);
            }
            return Poll::Pending;
        };
        TIMERS.with(|timers| {
            let mut timers = timers.borrow_mut();
            let waiter = timers.waiters.get_mut(&id).expect("owned timer waiter");
            if waiter.ready {
                Poll::Ready(())
            } else {
                if waiter
                    .waker
                    .as_ref()
                    .is_none_or(|waker| !waker.will_wake(cx.waker()))
                {
                    waiter.waker = Some(cx.waker().clone());
                }
                Poll::Pending
            }
        })
    }
}

impl Drop for BrowserTaskTimer {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            TIMERS.with(|timers| {
                timers.borrow_mut().waiters.remove(&id);
            });
            // SAFETY: Scalar cancellation removes only this exact queued JS timer.
            // No pointer is borrowed or retained and no callback reenters Rust.
            unsafe {
                timer_cancel(id);
            }
        }
    }
}

/// Timer callback wakes only the corresponding task's ready queue.
// SAFETY: Unique export accepts a scalar identity; no pointer or Host borrow occurs.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_task_timer_ready(id: u64) {
    let waker = TIMERS.with(|timers| {
        let mut timers = timers.borrow_mut();
        let waiter = timers.waiters.get_mut(&id)?;
        waiter.ready = true;
        waiter.waker.take()
    });
    if let Some(waker) = waker {
        waker.wake();
    }
}

/// Trusted read-only scenario observation; never advances a World.
#[cfg(feature = "instrumentation")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_test_asset_export_world_tick(world: u64) -> u64 {
    crate::BOUNDARY.with_borrow_mut(|boundary| boundary.asset_export_testing_tick(world))
}

/// Trusted scenario gate; graphics staging remains owned by the renderer.
#[cfg(all(feature = "instrumentation", feature = "render"))]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_test_asset_export_staging_gate(enabled: u32) {
    crate::BOUNDARY.with_borrow_mut(|boundary| boundary.asset_export_testing_gate(enabled != 0));
}
