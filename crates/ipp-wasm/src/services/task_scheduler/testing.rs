//! Trusted instrumentation probe executed through the actual browser worker adapter.

use std::cell::{Cell, RefCell};
use std::future::{Future, pending, poll_fn};
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use ipp_host_session::services::task_scheduler::TaskHandle;

use crate::services::WasmHost;

const SESSION: u64 = u64::MAX - 1;
const NOISY_POLLS: u32 = 256;

#[derive(Default)]
struct Gate {
    released: bool,
    waker: Option<Waker>,
}

#[derive(Default)]
struct Observations {
    polls: Cell<u32>,
    peer: Cell<u32>,
    io: Cell<u32>,
    returned: Cell<u32>,
}

struct Noisy(Rc<Observations>);

impl Future for Noisy {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let polls = self.0.polls.get() + 1;
        self.0.polls.set(polls);
        if polls == NOISY_POLLS {
            Poll::Ready(())
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

struct Probe {
    observations: Rc<Observations>,
    gate: Rc<RefCell<Gate>>,
    lease: Rc<()>,
    cancelled: Option<TaskHandle<()>>,
    _tasks: Vec<TaskHandle<()>>,
}

thread_local! {
    static PROBE: RefCell<Option<Probe>> = const { RefCell::new(None) };
}

pub(crate) fn clear() {
    PROBE.with_borrow_mut(|probe| {
        probe.take();
    });
}

pub(crate) fn start(host: &mut WasmHost) -> bool {
    clear();
    if host.open_session(SESSION, &[]).is_err() {
        return false;
    }
    let observations = Rc::new(Observations::default());
    let gate = Rc::new(RefCell::new(Gate::default()));
    let lease = Rc::new(());
    let schedulers = host.task_schedulers();
    let noisy = schedulers.host().spawn(Noisy(observations.clone()));
    let peer_observations = observations.clone();
    let peer = schedulers
        .host()
        .spawn(async move { peer_observations.peer.set(peer_observations.polls.get()) });
    let io_observations = observations.clone();
    let host_observations = observations.clone();
    let io_gate = gate.clone();
    // Rc across the I/O await proves that WASM I/O remains on this Host worker.
    let io = schedulers.io();
    let returned = schedulers.host().spawn(async move {
        io.spawn(async move {
            poll_fn(|cx| {
                let mut gate = io_gate.borrow_mut();
                if gate.released {
                    Poll::Ready(())
                } else {
                    gate.waker = Some(cx.waker().clone());
                    Poll::Pending
                }
            })
            .await;
            io_observations.io.set(1);
        })
        .await
        .expect("probe I/O task remains owned");
        host_observations.returned.set(1);
    });
    let pending_lease = lease.clone();
    let cancelled = schedulers.host().spawn(async move {
        let _lease = pending_lease;
        pending::<()>().await;
    });
    PROBE.with_borrow_mut(|probe| {
        *probe = Some(Probe {
            observations,
            gate,
            lease,
            cancelled: Some(cancelled),
            _tasks: vec![noisy, peer, returned],
        });
    });
    true
}

pub(crate) fn world_tick(host: &mut WasmHost) -> u64 {
    host.session_mut(SESSION)
        .map_or(0, |session| session.world().tick())
}

/// Start bounded/self-waking, local peer, gated I/O and pending cancellation tasks.
// SAFETY: Unique instrumentation symbol; no borrowed pointers cross the call.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_test_task_scheduler_start() -> u32 {
    crate::BOUNDARY.with_borrow_mut(|boundary| u32::from(boundary.scheduler_testing_start()))
}

/// Release the owned gate; its waker only requests a later worker service turn.
// SAFETY: Unique instrumentation symbol; no pointers or World references are accepted.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_test_task_scheduler_release() {
    let waker = PROBE.with_borrow(|probe| {
        probe.as_ref().and_then(|probe| {
            let mut gate = probe.gate.borrow_mut();
            gate.released = true;
            gate.waker.take()
        })
    });
    if let Some(waker) = waker {
        waker.wake();
    }
}

/// Drop the pending task's ownership without stepping Worlds.
// SAFETY: Unique instrumentation symbol; all task ownership stays on this worker.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_test_task_scheduler_cancel() {
    PROBE.with_borrow_mut(|probe| {
        if let Some(probe) = probe {
            probe.cancelled.take();
        }
    });
}

/// Probe counters, retained lease count and untouched World's tick.
// SAFETY: Unique instrumentation symbol; returns values without exposing pointers.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_test_task_scheduler_state(field: u32) -> u64 {
    if field == 5 {
        return crate::BOUNDARY.with_borrow_mut(|boundary| boundary.scheduler_testing_tick());
    }
    PROBE.with_borrow(|probe| {
        probe.as_ref().map_or(0, |probe| match field {
            0 => u64::from(probe.observations.polls.get()),
            1 => u64::from(probe.observations.peer.get()),
            2 => u64::from(probe.observations.io.get()),
            3 => u64::from(probe.observations.returned.get()),
            4 => Rc::strong_count(&probe.lease) as u64,
            _ => 0,
        })
    })
}
