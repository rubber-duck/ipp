use std::collections::VecDeque;
use std::future::Future;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

use super::IoScheduler;
use super::task::{Cancellation, TaskHandle, cancellable};

/// Maximum ready Host task polls in one update, including self-waking tasks.
const HOST_POLL_BUDGET: usize = 64;

type Wakeup = Arc<dyn Fn() + Send + Sync>;

pub(super) struct HostReadyQueue {
    ready: Mutex<VecDeque<async_task::Runnable>>,
    wakeup: Mutex<Option<Wakeup>>,
    closed: AtomicBool,
    tasks: Mutex<Vec<Weak<Cancellation>>>,
}

impl HostReadyQueue {
    fn schedule(&self, runnable: async_task::Runnable) {
        self.ready.lock().unwrap().push_back(runnable);
        // Clone outside the lock: platform callbacks may enqueue their own work.
        let wakeup = self.wakeup.lock().unwrap().clone();
        if let Some(wakeup) = wakeup {
            wakeup();
        }
    }
}

/// Local execution context. Futures may hold thread-local state across awaits.
#[derive(Clone)]
pub struct HostScheduler {
    queue: Arc<HostReadyQueue>,
    owner: PhantomData<Rc<()>>,
}

impl HostScheduler {
    /// Schedule a local future. The Host must remain alive and drive updates.
    pub fn spawn<F>(&self, future: F) -> TaskHandle<F::Output>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        if self.queue.closed.load(Ordering::Acquire) {
            return TaskHandle::cancelled();
        }
        let cancellation = Arc::new(Cancellation::default());
        {
            let mut tasks = self.queue.tasks.lock().unwrap();
            tasks.retain(|task| task.strong_count() != 0);
            tasks.push(Arc::downgrade(&cancellation));
            if self.queue.closed.load(Ordering::Acquire) {
                cancellation.cancel();
            }
        }
        let queue = self.queue.clone();
        let (runnable, task) =
            async_task::spawn_local(cancellable(future, cancellation.clone()), move |runnable| {
                queue.schedule(runnable)
            });
        runnable.schedule();
        TaskHandle {
            task: Some(task),
            cancellation,
        }
    }
}

/// Context handles shared by Host services without borrowing Worlds.
#[derive(Clone)]
pub struct TaskSchedulers {
    host: HostScheduler,
    io: IoScheduler,
}

impl TaskSchedulers {
    /// The Host thread execution context.
    pub fn host(&self) -> HostScheduler {
        self.host.clone()
    }

    /// Native worker execution, or the Host worker in browser WASM.
    pub fn io(&self) -> IoScheduler {
        self.io.clone()
    }
}

/// Owns scheduler lifetime, bounded Host polling, and native worker shutdown.
/// Drop cancels tasks and drains their future state before releasing services.
pub struct TaskSchedulerService {
    schedulers: TaskSchedulers,
    #[cfg(not(target_arch = "wasm32"))]
    workers: super::io::IoWorkers,
}

impl Default for TaskSchedulerService {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskSchedulerService {
    /// Initialize execution contexts without a World or platform event loop.
    pub fn new() -> Self {
        let host = HostScheduler {
            queue: Arc::new(HostReadyQueue {
                ready: Mutex::new(VecDeque::new()),
                wakeup: Mutex::new(None),
                closed: AtomicBool::new(false),
                tasks: Mutex::new(Vec::new()),
            }),
            owner: PhantomData,
        };
        #[cfg(not(target_arch = "wasm32"))]
        let (io, workers) = super::io::IoWorkers::new();
        #[cfg(target_arch = "wasm32")]
        let io = host.clone();
        Self {
            schedulers: TaskSchedulers {
                host,
                io,
            },
            #[cfg(not(target_arch = "wasm32"))]
            workers,
        }
    }

    /// Clone scheduling handles for owned service operations.
    pub fn schedulers(&self) -> TaskSchedulers {
        self.schedulers.clone()
    }

    /// Install an enqueue-only callback requesting a platform service update.
    pub fn set_wakeup(&mut self, wakeup: Wakeup) {
        *self.schedulers.host.queue.wakeup.lock().unwrap() = Some(wakeup.clone());
        #[cfg(not(target_arch = "wasm32"))]
        self.workers.set_wakeup(wakeup.clone());
        if self.has_ready_tasks() {
            wakeup();
        }
    }

    /// Poll a bounded turn before World work or during a service-only update.
    pub fn poll_ready(&mut self) -> usize {
        let mut polls = 0;
        while polls < HOST_POLL_BUDGET {
            let runnable = self.schedulers.host.queue.ready.lock().unwrap().pop_front();
            let Some(runnable) = runnable else {
                break;
            };
            runnable.run();
            polls += 1;
        }
        polls
    }

    /// Whether another bounded Host turn can make progress immediately.
    pub fn has_ready_tasks(&self) -> bool {
        !self.schedulers.host.queue.ready.lock().unwrap().is_empty()
    }

    /// Cancel owned work and release task futures, preserving external operation leases.
    pub fn shutdown(&mut self) {
        let queue = &self.schedulers.host.queue;
        queue.closed.store(true, Ordering::Release);
        let tasks: Vec<_> = queue
            .tasks
            .lock()
            .unwrap()
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        for task in tasks {
            task.cancel();
        }
        while self.has_ready_tasks() {
            self.poll_ready();
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.workers.shutdown();
    }
}

impl Drop for TaskSchedulerService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct AssetLoadTaskLease {
    _task: TaskHandle<()>,
}

impl ipp_core::services::asset_management::AssetLoadTask for AssetLoadTaskLease {}

impl ipp_core::services::asset_management::AssetLoadScheduler for HostScheduler {
    fn spawn(
        &self,
        future: std::pin::Pin<Box<dyn Future<Output = ()> + 'static>>,
    ) -> Box<dyn ipp_core::services::asset_management::AssetLoadTask> {
        Box::new(AssetLoadTaskLease {
            _task: HostScheduler::spawn(self, future),
        })
    }
}
