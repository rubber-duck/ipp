use super::*;
use std::cell::Cell;
use std::future::{Future, pending};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

struct SelfWaking(Arc<AtomicUsize>);

impl Future for SelfWaking {
    type Output = ();

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.0.fetch_add(1, Ordering::Relaxed);
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

#[test]
fn local_tasks_keep_thread_affinity_and_self_waking_work_is_bounded() {
    let mut service = TaskSchedulerService::new();
    let count = Arc::new(AtomicUsize::new(0));
    let noisy = service.schedulers().host().spawn(SelfWaking(count.clone()));
    let local = Rc::new(Cell::new(false));
    let completed = local.clone();
    let owner = std::thread::current().id();
    let task = service.schedulers().host().spawn(async move {
        assert_eq!(std::thread::current().id(), owner);
        completed.set(true);
    });
    assert_eq!(service.poll_ready(), 64);
    assert!(local.get(), "peer task progresses in the same bounded turn");
    assert_eq!(count.load(Ordering::Relaxed), 63);
    assert!(task.is_finished());
    drop(noisy);
    service.shutdown();
}

#[test]
fn dropping_handles_and_host_shutdown_release_pending_local_state() {
    let mut service = TaskSchedulerService::new();
    let owned = Rc::new(());
    let lease = owned.clone();
    let task = service.schedulers().host().spawn(async move {
        let _lease = lease;
        pending::<()>().await;
    });
    service.poll_ready();
    assert_eq!(Rc::strong_count(&owned), 2);
    drop(task);
    service.poll_ready();
    assert_eq!(Rc::strong_count(&owned), 1);
    let lease = owned.clone();
    let task = service.schedulers().host().spawn(async move {
        let _lease = lease;
        pending::<()>().await;
    });
    service.poll_ready();
    service.shutdown();
    assert_eq!(Rc::strong_count(&owned), 1);
    assert!(task.is_finished());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_io_await_preserves_host_context_and_shutdown_drains_pending_tasks() {
    let mut service = TaskSchedulerService::new();
    let owner = std::thread::current().id();
    let schedulers = service.schedulers();
    let finished = Rc::new(Cell::new(false));
    let complete = finished.clone();
    let host = schedulers.host().spawn(async move {
        assert_eq!(std::thread::current().id(), owner);
        let worker = schedulers
            .io()
            .spawn(async move { std::thread::current().id() })
            .await
            .unwrap();
        assert_ne!(worker, owner);
        assert_eq!(std::thread::current().id(), owner);
        complete.set(true);
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !finished.get() {
        assert!(std::time::Instant::now() < deadline);
        service.poll_ready();
        std::thread::yield_now();
    }
    assert!(host.is_finished());
    let io = service.schedulers().io().spawn(pending::<()>());
    service.shutdown();
    assert!(io.is_finished());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cancelled_blocking_work_retains_inputs_until_late_completion_and_closed_contexts_reject_spawn() {
    let mut service = TaskSchedulerService::new();
    let schedulers = service.schedulers();
    let (started, beginning) = std::sync::mpsc::sync_channel(1);
    let (release, released) = std::sync::mpsc::sync_channel(1);
    let (completed, ending) = std::sync::mpsc::sync_channel(1);
    let owned = Arc::new(());
    let operation_lease = owned.clone();
    let task = schedulers.io().blocking(move || {
        let lease = operation_lease;
        started.send(()).unwrap();
        released.recv().unwrap();
        drop(lease);
        completed.send(()).unwrap();
    });
    beginning
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    service.shutdown();
    assert_eq!(
        Arc::strong_count(&owned),
        2,
        "late operation still owns its input"
    );
    assert_eq!(async_io::block_on(task), Err(TaskCancelled));
    release.send(()).unwrap();
    ending
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    assert_eq!(Arc::strong_count(&owned), 1);
    let host_task = schedulers
        .host()
        .spawn(async { panic!("closed Host cannot run") });
    assert_eq!(async_io::block_on(host_task), Err(TaskCancelled));
    let io_task = schedulers
        .io()
        .spawn(async { panic!("closed I/O cannot run") });
    assert_eq!(async_io::block_on(io_task), Err(TaskCancelled));
}
