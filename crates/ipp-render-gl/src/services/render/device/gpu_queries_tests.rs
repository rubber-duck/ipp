use super::*;
use std::collections::BTreeMap;

struct Backend {
    capability: RenderGpuCapability,
    ready: bool,
    disjoint: bool,
    lost: bool,
    next: u32,
    values: BTreeMap<u32, u64>,
    deleted: Vec<u32>,
    active: bool,
    reads: usize,
}

impl Backend {
    fn new(capability: RenderGpuCapability) -> Self {
        Self {
            capability,
            ready: false,
            disjoint: false,
            lost: false,
            next: 0,
            values: BTreeMap::new(),
            deleted: Vec::new(),
            active: false,
            reads: 0,
        }
    }
}

impl GpuQueryBackend for Backend {
    fn capability(&self) -> RenderGpuCapability {
        self.capability
    }

    fn create(&mut self) -> Option<u32> {
        self.next += 1;
        self.values.insert(self.next, 0);
        Some(self.next)
    }

    fn delete(&mut self, query: u32) {
        self.deleted.push(query);
    }

    fn timestamp(&mut self, query: u32) {
        self.values.insert(query, u64::from(query) * 100);
    }

    fn begin_elapsed(&mut self, _query: u32) {
        assert!(!self.active);
        self.active = true;
    }

    fn end_elapsed(&mut self) {
        assert!(self.active);
        self.active = false;
    }

    fn available(&mut self, _query: u32) -> bool {
        self.ready
    }

    fn result(&mut self, query: u32) -> Option<u64> {
        assert!(self.ready);
        self.reads += 1;
        self.values.get(&query).copied()
    }

    fn disjoint(&mut self) -> bool {
        std::mem::take(&mut self.disjoint)
    }

    fn context_lost(&mut self) -> bool {
        self.lost
    }
}

#[test]
fn pending_never_reads_results_and_zero_is_a_valid_elapsed_result() {
    let mut pool = GpuQueryPool::new(Backend::new(RenderGpuCapability::Elapsed));
    let token = pool.start().unwrap();
    pool.end(token);
    assert_eq!(pool.poll(token), RenderGpuAvailability::Pending);
    assert_eq!(pool.backend.reads, 0);
    pool.backend.ready = true;
    assert_eq!(
        pool.poll(token),
        RenderGpuAvailability::Available {
            duration_ns: 0
        }
    );
    assert_eq!(pool.backend.deleted, [1]);
}

#[test]
fn timestamps_allow_nested_scopes_and_preserve_each_pair() {
    let mut pool = GpuQueryPool::new(Backend::new(RenderGpuCapability::Timestamps));
    let frame = pool.start().unwrap();
    let pass = pool.start().unwrap();
    pool.end(pass);
    pool.end(frame);
    pool.backend.ready = true;
    assert_eq!(
        pool.poll(frame),
        RenderGpuAvailability::Available {
            duration_ns: 100
        }
    );
    assert_eq!(
        pool.poll(pass),
        RenderGpuAvailability::Available {
            duration_ns: 100
        }
    );
    assert_eq!(pool.backend.reads, 4);
}

#[test]
fn elapsed_overlap_and_capacity_drop_leave_render_work_untouched() {
    let mut pool = GpuQueryPool::new(Backend::new(RenderGpuCapability::Elapsed));
    pool.capacity = 2;
    let token = pool.start().unwrap();
    assert_eq!(pool.start(), Err(RenderGpuAvailability::OverlapSkipped));
    pool.end(token);
    assert_eq!(pool.start(), Err(RenderGpuAvailability::CapacityDropped));
    pool.backend.ready = true;
    pool.poll(token);
    let next = pool.start().unwrap();
    assert_ne!(next, token);
    assert_eq!(pool.poll(token), RenderGpuAvailability::Stopped);
}

#[test]
fn invalid_result_is_unavailable_and_token_exhaustion_allocates_nothing() {
    let mut pool = GpuQueryPool::new(Backend::new(RenderGpuCapability::Elapsed));
    let token = pool.start().unwrap();
    pool.end(token);
    pool.backend.ready = true;
    pool.backend.values.clear();
    assert_eq!(pool.poll(token), RenderGpuAvailability::Unsupported);
    assert_eq!(pool.backend.deleted, [1]);

    pool.next_token = u64::MAX;
    let allocated = pool.backend.next;
    assert_eq!(pool.start(), Err(RenderGpuAvailability::CapacityDropped));
    assert_eq!(pool.backend.next, allocated);
    assert!(!pool.backend.active);
}

#[test]
fn disjoint_invalidates_all_pending_queries_without_readback() {
    let mut pool = GpuQueryPool::new(Backend::new(RenderGpuCapability::Timestamps));
    let first = pool.start().unwrap();
    let second = pool.start().unwrap();
    pool.end(first);
    pool.end(second);
    pool.backend.disjoint = true;
    assert_eq!(pool.poll(first), RenderGpuAvailability::Disjoint);
    assert_eq!(pool.poll(second), RenderGpuAvailability::Disjoint);
    assert_eq!(pool.backend.reads, 0);
    assert_eq!(pool.backend.deleted.len(), 4);
}

#[test]
fn loss_abandons_names_and_stop_ends_and_deletes_active_elapsed_queries() {
    let mut pool = GpuQueryPool::new(Backend::new(RenderGpuCapability::Elapsed));
    let first = pool.start().unwrap();
    pool.stop(RenderGpuAvailability::Stopped);
    assert_eq!(pool.poll(first), RenderGpuAvailability::Stopped);
    assert!(!pool.backend.active);
    assert_eq!(pool.backend.deleted, [1]);
    let second = pool.start().unwrap();
    pool.backend.lost = true;
    assert_eq!(pool.poll(second), RenderGpuAvailability::ContextLost);
    assert_eq!(pool.backend.deleted, [1]);
    assert_eq!(pool.start(), Err(RenderGpuAvailability::ContextLost));
}
