use ipp_core::{
    HostRuntime,
    services::asset_management::{
        Asset, AssetLoadScheduler, AssetLoadTask, AssetManagementService, AsyncAssetLoader,
    },
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, VecDeque},
    future::Future,
    num::NonZeroUsize,
    pin::Pin,
    rc::{Rc, Weak},
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake, Waker},
};

type LoadFuture = Pin<Box<dyn Future<Output = ()>>>;
type Tasks = RefCell<BTreeMap<u64, Rc<RefCell<Option<LoadFuture>>>>>;

#[derive(Default)]
struct ReadyQueue(Mutex<VecDeque<u64>>);

struct TaskWake {
    id: u64,
    queue: Arc<ReadyQueue>,
}

impl Wake for TaskWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.queue.0.lock().unwrap().push_back(self.id);
    }
}

#[derive(Default)]
pub struct TestAssetScheduler {
    next: Cell<u64>,
    ready: Arc<ReadyQueue>,
    tasks: Rc<Tasks>,
}

struct TaskLease {
    id: u64,
    tasks: Weak<Tasks>,
}

impl AssetLoadTask for TaskLease {}

impl Drop for TaskLease {
    fn drop(&mut self) {
        if let Some(tasks) = self.tasks.upgrade() {
            tasks.borrow_mut().remove(&self.id);
        }
    }
}

impl AssetLoadScheduler for TestAssetScheduler {
    fn spawn(&self, future: LoadFuture) -> Box<dyn AssetLoadTask> {
        let id = self.next.get().checked_add(1).expect("test task identity");
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

impl TestAssetScheduler {
    pub fn poll_ready(&self) {
        for _ in 0..4096 {
            let Some(id) = self.ready.0.lock().unwrap().pop_front() else {
                return;
            };
            let Some(task) = self.tasks.borrow().get(&id).cloned() else {
                continue;
            };
            let waker = Waker::from(Arc::new(TaskWake {
                id,
                queue: self.ready.clone(),
            }));
            let mut task = task.borrow_mut();
            if let Some(future) = &mut *task
                && future.as_mut().poll(&mut Context::from_waker(&waker)) == Poll::Ready(())
            {
                *task = None;
            }
        }
        panic!("test executor did not quiesce");
    }
}

thread_local! { static SCHEDULERS: RefCell<Vec<Weak<TestAssetScheduler>>> = const { RefCell::new(Vec::new()) }; }

pub fn scheduler() -> Rc<TestAssetScheduler> {
    let scheduler = Rc::new(TestAssetScheduler::default());
    SCHEDULERS.with(|all| all.borrow_mut().push(Rc::downgrade(&scheduler)));
    scheduler
}

pub fn install(host: &mut HostRuntime) {
    host.set_asset_load_scheduler(scheduler());
}

pub fn install_assets(assets: &mut AssetManagementService) {
    assets.set_load_scheduler(scheduler());
}

pub fn host() -> HostRuntime {
    let mut host = HostRuntime::new();
    install(&mut host);
    host
}

pub fn poll_ready() {
    let schedulers: Vec<_> = SCHEDULERS.with(|all| {
        let mut all = all.borrow_mut();
        all.retain(|item| item.strong_count() != 0);
        all.iter().filter_map(Weak::upgrade).collect()
    });
    for scheduler in schedulers {
        scheduler.poll_ready();
    }
}

/// Blob fixtures deliberately retain bytes as their actual final representation.
pub fn blob_loader<T: Asset>(
    decode: impl FnOnce(&[u8]) -> Result<T, String> + 'static,
) -> AsyncAssetLoader<T> {
    AsyncAssetLoader::decode(move |mut reader| async move {
        let mut bytes = Vec::new();
        loop {
            let window = reader.read(NonZeroUsize::new(1).unwrap()).await?;
            let count = window.bytes().len();
            let done = window.is_final();
            bytes
                .try_reserve(count)
                .map_err(|error| error.to_string())?;
            bytes.extend_from_slice(window.bytes());
            window.consume(count)?;
            if done {
                break;
            }
        }
        decode(&bytes)
    })
}

pub fn with_factories(
    factories: Vec<Arc<dyn ipp_core::systems::SystemFactory>>,
) -> Result<HostRuntime, ipp_core::systems::SystemScheduleError> {
    let mut host = HostRuntime::with_system_factories(factories)?;
    install(&mut host);
    Ok(host)
}

/// Drive asset demand and real ready tasks without advancing any World.
pub fn progress(host: &mut HostRuntime) {
    host.progress_assets();
    poll_ready();
    host.progress_assets();
}

/// The test event loop explicitly polls its supplied executor around Host work.
pub trait HostTaskTestDriver {
    fn progress_assets_for_test(&mut self);

    fn frame_for_test(
        &mut self,
        delta: f64,
    ) -> Result<ipp_core::HostFrameReport, ipp_core::ErrorReason>;
}

impl HostTaskTestDriver for HostRuntime {
    fn progress_assets_for_test(&mut self) {
        progress(self);
    }

    fn frame_for_test(
        &mut self,
        delta: f64,
    ) -> Result<ipp_core::HostFrameReport, ipp_core::ErrorReason> {
        progress(self);
        let frame = self.frame(delta)?;
        progress(self);
        Ok(frame)
    }
}

pub trait WorldTaskTestDriver {
    fn poll_assets_for_test(&mut self);
}

impl WorldTaskTestDriver for ipp_core::WorldContext<'_> {
    fn poll_assets_for_test(&mut self) {
        self.poll_assets();
        poll_ready();
        self.poll_assets();
    }
}
