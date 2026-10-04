//! Host-owned asset task progression for synchronous native graphics examples.
//!
//! Install before acquiring resources and poll at the example's service phases.
//! Real readiness wakers enqueue work; dropping a resource task lease cancels it.

use ipp_core::{
    HostRuntime,
    services::asset_management::{AssetLoadScheduler, AssetLoadTask},
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, VecDeque},
    future::Future,
    pin::Pin,
    rc::{Rc, Weak},
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake, Waker},
};

type LoadFuture = Pin<Box<dyn Future<Output = ()>>>;
type TaskSlot = Rc<RefCell<Option<LoadFuture>>>;
type TaskTable = RefCell<BTreeMap<u64, TaskSlot>>;

#[derive(Default)]
struct ReadyQueue(Mutex<VecDeque<u64>>);

struct TaskWake {
    id: u64,
    ready: Arc<ReadyQueue>,
}

impl Wake for TaskWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.0.lock().unwrap().push_back(self.id);
    }
}

#[derive(Default)]
pub(crate) struct SmokeAssetTasks {
    next: Cell<u64>,
    ready: Arc<ReadyQueue>,
    tasks: Rc<TaskTable>,
}

struct TaskLease {
    id: u64,
    tasks: Weak<TaskTable>,
}

impl AssetLoadTask for TaskLease {}

impl Drop for TaskLease {
    fn drop(&mut self) {
        if let Some(tasks) = self.tasks.upgrade() {
            tasks.borrow_mut().remove(&self.id);
        }
    }
}

impl AssetLoadScheduler for SmokeAssetTasks {
    fn spawn(&self, future: LoadFuture) -> Box<dyn AssetLoadTask> {
        let id = self.next.get().checked_add(1).expect("asset task identity");
        self.next.set(id);
        self.tasks
            .borrow_mut()
            .insert(id, Rc::new(RefCell::new(Some(future))));
        self.ready.0.lock().unwrap().push_back(id);
        Box::new(TaskLease {
            id,
            tasks: Rc::downgrade(&self.tasks),
        })
    }
}

impl SmokeAssetTasks {
    pub(crate) fn install(host: &mut HostRuntime) -> Rc<Self> {
        let tasks = Rc::new(Self::default());
        host.set_asset_load_scheduler(tasks.clone());
        tasks
    }

    /// Bound each service phase; further ready work continues on the next frame.
    pub(crate) fn poll_ready(&self) {
        for _ in 0..4096 {
            let Some(id) = self.ready.0.lock().unwrap().pop_front() else {
                return;
            };
            let Some(task) = self.tasks.borrow().get(&id).cloned() else {
                continue;
            };
            let waker = Waker::from(Arc::new(TaskWake {
                id,
                ready: self.ready.clone(),
            }));
            let mut task = task.borrow_mut();
            if let Some(future) = &mut *task
                && future.as_mut().poll(&mut Context::from_waker(&waker)) == Poll::Ready(())
            {
                *task = None;
            }
        }
    }
}
