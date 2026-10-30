use super::*;
use crate::services::task_scheduler::TaskSchedulerService;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Poll, Waker};

#[test]
fn concurrent_wakes_and_shutdown_reuse_reserved_task_slots() {
    let mut service = TaskSchedulerService::new();
    let io = service.schedulers().io();
    let wakers = Arc::new(Mutex::new(vec![None::<Waker>; 64]));
    let dropped = Arc::new(AtomicUsize::new(0));

    struct Lease(Arc<AtomicUsize>);

    impl Drop for Lease {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Release);
        }
    }

    let tasks: Vec<_> = (0..64)
        .map(|index| {
            let wakers = wakers.clone();
            let lease = Lease(dropped.clone());
            io.spawn(async move {
                let _lease = lease;
                std::future::poll_fn(|cx| {
                    wakers.lock().unwrap()[index] = Some(cx.waker().clone());
                    Poll::<()>::Pending
                })
                .await;
            })
        })
        .collect();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while wakers.lock().unwrap().iter().any(Option::is_none) {
        assert!(
            std::time::Instant::now() < deadline,
            "workers registered their waits"
        );
        std::thread::yield_now();
    }
    let wakers: Vec<_> = wakers.lock().unwrap().iter().flatten().cloned().collect();
    let capacity = io.execution.queue.lock().unwrap().ready.capacity();
    assert!(capacity >= tasks.len());

    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                for _ in 0..256 {
                    for waker in &wakers {
                        waker.wake_by_ref();
                    }
                }
            });
        }
        // Cancellation races with cross-thread wakes; stale wakers can outlive
        // shutdown but must never resurrect or enqueue a completed future.
        service.shutdown();
    });
    assert_eq!(dropped.load(Ordering::Acquire), tasks.len());
    let queue = io.execution.queue.lock().unwrap();
    assert_eq!(queue.active, 0);
    assert!(queue.ready.is_empty());
    assert_eq!(queue.ready.capacity(), capacity);
}

#[test]
fn task_panics_reach_the_handle_without_terminating_io_workers() {
    let mut service = TaskSchedulerService::new();
    for _ in 0..8 {
        let task = service
            .schedulers()
            .io()
            .spawn(async { panic!("task panic") });
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                async_io::block_on(task)
            }))
            .is_err()
        );
    }
    assert_eq!(
        async_io::block_on(service.schedulers().io().spawn(async { 7 })),
        Ok(7)
    );
    service.shutdown();
}
