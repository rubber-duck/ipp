use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::task::Wake;

#[derive(Default)]
struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn lost_context_waits_once_and_cancelled_waiters_are_removed() {
    let context = RenderAssetContext::default();
    context.set_active(false);
    let count = Arc::new(WakeCount::default());
    let waker = Waker::from(count.clone());
    let mut cx = Context::from_waker(&waker);
    let mut wait = context.wait();
    assert!(Pin::new(&mut wait).poll(&mut cx).is_pending());
    assert!(Pin::new(&mut wait).poll(&mut cx).is_pending());
    assert_eq!(context.0.borrow().waiters.len(), 1);
    assert_eq!(count.0.load(Ordering::SeqCst), 0);
    drop(wait);
    assert!(context.0.borrow().waiters.is_empty());
    context.set_active(true);
    assert_eq!(count.0.load(Ordering::SeqCst), 0);
}

#[test]
fn restoration_wakes_live_waiters_and_never_revives_old_gpu_leases() {
    let context = RenderAssetContext::default();
    let old = context.lease();
    assert!(old.is_current());
    context.set_active(false);
    assert!(!old.is_current());
    let count = Arc::new(WakeCount::default());
    let waker = Waker::from(count.clone());
    let mut cx = Context::from_waker(&waker);
    let mut wait = context.wait();
    assert!(Pin::new(&mut wait).poll(&mut cx).is_pending());
    context.set_active(true);
    assert_eq!(count.0.load(Ordering::SeqCst), 1);
    let Poll::Ready(new) = Pin::new(&mut wait).poll(&mut cx) else {
        panic!("restored context still pending")
    };
    assert!(new.is_current());
    assert!(!old.is_current());
    assert!(context.0.borrow().waiters.is_empty());
}
