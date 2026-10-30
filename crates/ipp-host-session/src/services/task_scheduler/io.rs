//! Native Smol tasks on a Host-owned ready queue. Each live future reserves one
//! queue slot at spawn; async-task coalesces repeated wakes into one Runnable.
//! Queue capacity survives completion and is reused by later tasks. Workers park
//! on a condition variable; async-io keeps its own socket/timer reactor thread.
//! See <https://docs.rs/async-task/4.7.1/async_task/> and
//! <https://docs.rs/async-io/2.6.0/async_io/#implementation>.

use std::collections::VecDeque;
use std::future::Future;
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread::JoinHandle;

use super::task::{Cancellation, TaskHandle, cancellable};

#[cfg(test)]
#[path = "io_tests.rs"]
mod tests;

type Wakeup = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct IoReadyQueue {
    ready: VecDeque<async_task::Runnable>,
    tasks: Vec<Weak<Cancellation>>,
    active: usize,
    closed: bool,
    stopped: bool,
}

struct IoExecution {
    queue: Mutex<IoReadyQueue>,
    ready: Condvar,
    drained: Condvar,
    wakeup: Mutex<Option<Wakeup>>,
}

impl IoExecution {
    fn schedule(&self, runnable: async_task::Runnable) {
        let mut queue = self.queue.lock().unwrap();
        // async-task coalesces wakes: at most one Runnable for each live task.
        // Spawn reserves that task's slot before making it schedulable, so wake
        // paths never grow storage, including self-wakes and cancellation.
        assert!(
            queue.ready.len() < queue.ready.capacity(),
            "reserved I/O ready slot"
        );
        queue.ready.push_back(runnable);
        self.ready.notify_one();
    }

    fn next(&self) -> Option<async_task::Runnable> {
        let mut queue = self.queue.lock().unwrap();
        loop {
            if let Some(runnable) = queue.ready.pop_front() {
                return Some(runnable);
            }
            if queue.stopped {
                return None;
            }
            queue = self.ready.wait(queue).unwrap();
        }
    }
}

struct IoTaskLifetime(Arc<IoExecution>);

impl Drop for IoTaskLifetime {
    fn drop(&mut self) {
        let mut queue = self.0.queue.lock().unwrap();
        queue.active -= 1;
        self.0.drained.notify_all();
        let wakeup = self.0.wakeup.lock().unwrap().clone();
        drop(queue);
        if let Some(wakeup) = wakeup {
            wakeup();
        }
    }
}

/// Native I/O execution context with explicit thread-safe transfer bounds.
#[derive(Clone)]
pub struct IoScheduler {
    execution: Arc<IoExecution>,
}

impl IoScheduler {
    /// Execute an owned Send future on native workers.
    pub fn spawn<F>(&self, future: F) -> TaskHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let cancellation = Arc::new(Cancellation::default());
        let mut queue = self.execution.queue.lock().unwrap();
        if queue.closed {
            return TaskHandle::cancelled();
        }
        queue.tasks.retain(|task| task.strong_count() != 0);
        queue.tasks.push(Arc::downgrade(&cancellation));
        queue.active += 1;
        let additional = queue.active.saturating_sub(queue.ready.len());
        queue.ready.reserve(additional);
        let lifetime = IoTaskLifetime(self.execution.clone());
        drop(queue);

        let cancelled = cancellation.clone();
        let execution = self.execution.clone();
        let (runnable, task) = async_task::Builder::new().propagate_panic(true).spawn(
            |()| async move {
                let _lifetime = lifetime;
                cancellable(future, cancelled).await
            },
            move |runnable| execution.schedule(runnable),
        );
        runnable.schedule();
        TaskHandle {
            task: Some(task),
            cancellation,
        }
    }

    /// Run owned blocking work in Smol's pool. Cancellation never borrows released state.
    pub fn blocking<F, T>(&self, operation: F) -> TaskHandle<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        self.spawn(blocking::unblock(operation))
    }
}

pub(super) struct IoWorkers {
    execution: Arc<IoExecution>,
    workers: Vec<JoinHandle<()>>,
}

impl IoWorkers {
    pub(super) fn new() -> (IoScheduler, Self) {
        let execution = Arc::new(IoExecution {
            queue: Mutex::new(IoReadyQueue::default()),
            ready: Condvar::new(),
            drained: Condvar::new(),
            wakeup: Mutex::new(None),
        });
        let worker_count =
            std::thread::available_parallelism().map_or(1, |count| count.get().min(4));
        let workers = (0..worker_count)
            .map(|index| {
                let execution = execution.clone();
                std::thread::Builder::new()
                    .name(format!("ipp-io-{index}"))
                    .spawn(move || {
                        // async-io's reactor thread handles socket readiness.
                        // Ready tasks park here without waking kqueue just to poll
                        // a future that the Host has already made runnable.
                        while let Some(runnable) = execution.next() {
                            runnable.run();
                        }
                    })
                    .expect("native I/O worker creation failed")
            })
            .collect();
        (
            IoScheduler {
                execution: execution.clone(),
            },
            Self {
                execution,
                workers,
            },
        )
    }

    pub(super) fn set_wakeup(&mut self, wakeup: Wakeup) {
        *self.execution.wakeup.lock().unwrap() = Some(wakeup);
    }

    pub(super) fn shutdown(&mut self) {
        if self.workers.is_empty() {
            return;
        }
        let tasks: Vec<_> = {
            let mut queue = self.execution.queue.lock().unwrap();
            queue.closed = true;
            queue.tasks.iter().filter_map(Weak::upgrade).collect()
        };
        for task in tasks {
            task.cancel();
        }
        let mut queue = self.execution.queue.lock().unwrap();
        while queue.active != 0 {
            queue = self.execution.drained.wait(queue).unwrap();
        }
        queue.stopped = true;
        self.execution.ready.notify_all();
        drop(queue);
        for worker in self.workers.drain(..) {
            worker.join().expect("native I/O worker panicked");
        }
    }
}
