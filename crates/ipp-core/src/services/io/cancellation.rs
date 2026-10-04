//! Owned cancellation fences shared by registrations and acquisition.

use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};

#[derive(Default)]
struct IoCancellationState {
    cancelled: AtomicBool,
    wakers: Mutex<Vec<Waker>>,
    children: Mutex<Vec<Weak<IoCancellationState>>>,
    observers: Mutex<Vec<Weak<Mutex<Option<Waker>>>>>,
}

/// Cancellation fence preserving outstanding backing and waking pending work.
#[derive(Clone, Default)]
pub struct IoCancellation(Arc<IoCancellationState>);

impl IoCancellation {
    /// Revoke future progress; this never undoes bytes already accepted.
    pub fn cancel(&self) {
        if self.0.cancelled.swap(true, Ordering::AcqRel) {
            return;
        }
        let wakers = std::mem::take(&mut *self.0.wakers.lock().expect("IO cancellation lock"));
        for waker in wakers {
            waker.wake();
        }
        let observers =
            std::mem::take(&mut *self.0.observers.lock().expect("IO cancellation observers"));
        for observer in observers
            .into_iter()
            .filter_map(|observer| observer.upgrade())
        {
            let waker = observer.lock().expect("IO cancellation observer").take();
            if let Some(waker) = waker {
                waker.wake();
            }
        }
        let children =
            std::mem::take(&mut *self.0.children.lock().expect("IO cancellation children"));
        for child in children.into_iter().filter_map(|child| child.upgrade()) {
            Self(child).cancel();
        }
    }

    /// Propagate cancellation to an acquisition without owning its storage.
    /// Link only parent-to-child fences; a child never revokes its parent.
    pub fn link(&self, child: &Self) {
        if Arc::ptr_eq(&self.0, &child.0) {
            return;
        }
        let mut children = self.0.children.lock().expect("IO cancellation children");
        if self.is_cancelled() {
            drop(children);
            child.cancel();
        } else {
            children.retain(|child| child.strong_count() != 0);
            if !children
                .iter()
                .any(|existing| existing.ptr_eq(&Arc::downgrade(&child.0)))
            {
                children.push(Arc::downgrade(&child.0));
            }
        }
    }

    /// Whether the originating operation or registration has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }

    /// Register a readiness wake without racing cancellation.
    pub fn register(&self, waker: &Waker) {
        let mut wakers = self.0.wakers.lock().expect("IO cancellation lock");
        if self.is_cancelled() {
            drop(wakers);
            waker.wake_by_ref();
        } else if !wakers.iter().any(|existing| existing.will_wake(waker)) {
            wakers.push(waker.clone());
        }
    }

    /// Own one readiness registration for a reader or operation. Dropping it
    /// immediately releases its task waker; long-lived grants retain no dead tasks.
    pub fn waiter(&self) -> IoCancellationWaiter {
        let observer = Arc::new(Mutex::new(None));
        let mut observers = self.0.observers.lock().expect("IO cancellation observers");
        observers.retain(|observer| observer.strong_count() != 0);
        observers.push(Arc::downgrade(&observer));
        IoCancellationWaiter {
            cancellation: self.clone(),
            observer,
        }
    }

    /// Await cancellation independently of simulation frames.
    pub fn cancelled(&self) -> IoCancelledFuture {
        IoCancelledFuture(self.waiter())
    }
}

/// Awaitable cancellation observation.
pub struct IoCancelledFuture(IoCancellationWaiter);

/// Owned readiness slot: one allocation per owned operation or reader, never
/// one allocation per read future. Updating the task waker releases its predecessor.
pub struct IoCancellationWaiter {
    cancellation: IoCancellation,
    observer: Arc<Mutex<Option<Waker>>>,
}

impl IoCancellationWaiter {
    /// Register/update the current task without retaining previous execution contexts.
    pub fn register(&self, waker: &Waker) {
        *self.observer.lock().expect("IO cancellation observer") = Some(waker.clone());
        if self.cancellation.is_cancelled() {
            waker.wake_by_ref();
        }
    }
}

impl Future for IoCancelledFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.0.register(cx.waker());
        if self.0.cancellation.is_cancelled() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
