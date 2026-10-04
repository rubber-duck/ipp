use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread::JoinHandle;

use super::task::{Cancellation, TaskHandle, cancellable};

type Wakeup = Arc<dyn Fn() + Send + Sync>;

struct IoExecution {
    executor: async_executor::Executor<'static>,
    closed: AtomicBool,
    tasks: Mutex<Vec<Weak<Cancellation>>>,
    active: Mutex<usize>,
    drained: Condvar,
    wakeup: Mutex<Option<Wakeup>>,
}

struct IoTaskLifetime(Arc<IoExecution>);

impl Drop for IoTaskLifetime {
    fn drop(&mut self) {
        let mut active = self.0.active.lock().unwrap();
        *active -= 1;
        self.0.drained.notify_all();
        let wakeup = self.0.wakeup.lock().unwrap().clone();
        drop(active);
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
        let mut tasks = self.execution.tasks.lock().unwrap();
        if self.execution.closed.load(Ordering::Acquire) {
            return TaskHandle::cancelled();
        }
        tasks.retain(|task| task.strong_count() != 0);
        tasks.push(Arc::downgrade(&cancellation));
        if self.execution.closed.load(Ordering::Acquire) {
            cancellation.cancel();
        }
        *self.execution.active.lock().unwrap() += 1;
        let lifetime = IoTaskLifetime(self.execution.clone());
        let cancelled = cancellation.clone();
        let task = self.execution.executor.spawn(async move {
            let _lifetime = lifetime;
            cancellable(future, cancelled).await
        });
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
    stop: async_channel::Sender<()>,
    workers: Vec<JoinHandle<()>>,
}

impl IoWorkers {
    pub(super) fn new() -> (IoScheduler, Self) {
        let execution = Arc::new(IoExecution {
            executor: async_executor::Executor::new(),
            closed: AtomicBool::new(false),
            tasks: Mutex::new(Vec::new()),
            active: Mutex::new(0),
            drained: Condvar::new(),
            wakeup: Mutex::new(None),
        });
        let (stop, stopped) = async_channel::bounded::<()>(1);
        let worker_count =
            std::thread::available_parallelism().map_or(1, |count| count.get().min(4));
        let workers = (0..worker_count)
            .map(|index| {
                let execution = execution.clone();
                let stopped = stopped.clone();
                std::thread::Builder::new()
                    .name(format!("ipp-io-{index}"))
                    .spawn(move || {
                        let _ = async_io::block_on(execution.executor.run(stopped.recv()));
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
                stop,
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
        self.execution.closed.store(true, Ordering::Release);
        let tasks: Vec<_> = self
            .execution
            .tasks
            .lock()
            .unwrap()
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        for task in tasks {
            task.cancel();
        }
        let mut active = self.execution.active.lock().unwrap();
        while *active != 0 {
            active = self.execution.drained.wait(active).unwrap();
        }
        drop(active);
        self.stop.close();
        for worker in self.workers.drain(..) {
            worker.join().expect("native I/O worker panicked");
        }
    }
}
