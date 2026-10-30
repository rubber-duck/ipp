//! All-thread allocation accounting for repeated native task wakeups.
#![cfg(not(target_arch = "wasm32"))]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};
use std::time::{Duration, Instant};

use ipp_host_session::services::task_scheduler::TaskSchedulerService;

static CAPTURE: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

struct Allocator;

fn count() {
    if CAPTURE.load(Ordering::Relaxed) {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    }
}

// SAFETY: Every operation forwards the caller's unchanged allocator contract to
// System. Counters neither borrow nor retain any allocated storage.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: The caller supplies the valid layout; System owns allocation.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: The caller supplies the valid layout; System owns allocation.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        // SAFETY: The live pointer, original layout and new size are forwarded
        // unchanged. No reference to the potentially invalidated storage is kept.
        unsafe { System.realloc(pointer, layout, size) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: The caller exclusively releases this System allocation with
        // its original layout; this wrapper retains no aliases or pointer.
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

#[derive(Default)]
struct Probe {
    requested: AtomicUsize,
    completed: AtomicUsize,
    waker: Mutex<Option<Waker>>,
}

#[test]
fn warmed_native_io_wakes_allocate_on_neither_sender_nor_workers() {
    let mut scheduler = TaskSchedulerService::new();
    let probes: Vec<_> = (0..64).map(|_| Arc::new(Probe::default())).collect();
    let tasks: Vec<_> = probes
        .iter()
        .map(|probe| {
            let probe = probe.clone();
            scheduler
                .schedulers()
                .io()
                .spawn(std::future::poll_fn(move |cx| {
                    let mut waker = probe.waker.lock().unwrap();
                    if waker.is_none() {
                        *waker = Some(cx.waker().clone());
                    }
                    probe
                        .completed
                        .store(probe.requested.load(Ordering::Acquire), Ordering::Release);
                    Poll::<()>::Pending
                }))
        })
        .collect();
    let deadline = Instant::now() + Duration::from_secs(10);
    let wakers: Vec<_> = probes
        .iter()
        .map(|probe| {
            loop {
                if let Some(waker) = probe.waker.lock().unwrap().clone() {
                    break waker;
                }
                assert!(Instant::now() < deadline, "I/O task did not start");
                std::thread::yield_now();
            }
        })
        .collect();
    let mut delivered = 0;
    let mut notify = || {
        let index = delivered % probes.len();
        let probe = &probes[index];
        let requested = probe.requested.fetch_add(1, Ordering::Release) + 1;
        wakers[index].wake_by_ref();
        delivered += 1;
        while probe.completed.load(Ordering::Acquire) != requested {
            assert!(Instant::now() < deadline, "I/O task notification stalled");
            std::thread::yield_now();
        }
    };
    for _ in 0..1024 {
        notify();
    }

    ALLOCATIONS.store(0, Ordering::Relaxed);
    CAPTURE.store(true, Ordering::SeqCst);
    for _ in 0..4096 {
        notify();
    }
    CAPTURE.store(false, Ordering::SeqCst);
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);

    scheduler.shutdown();
    assert!(tasks.iter().all(|task| task.is_finished()));
    assert_eq!(
        allocations, 0,
        "ready-task wakeups must reuse their reserved storage"
    );
}
