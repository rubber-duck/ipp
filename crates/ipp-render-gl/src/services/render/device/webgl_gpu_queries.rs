//! Physical WASM timer bindings, absent from ordinary artifacts.
use super::RenderGpuCapability;
use super::gpu_queries::GpuQueryBackend;

#[link(wasm_import_module = "ipp_gl")]
unsafe extern "C" {
    fn gpu_query_capability() -> u32;
    fn gpu_query_timestamp_bits() -> u32;
    fn gpu_query_create() -> u32;
    fn gpu_query_delete(query: u32);
    fn gpu_query_timestamp(query: u32);
    fn gpu_query_begin(query: u32);
    fn gpu_query_end();
    fn gpu_query_available(query: u32) -> u32;
    fn gpu_query_result(query: u32) -> f64;
    fn gpu_query_disjoint() -> u32;
    fn gpu_query_context_lost() -> u32;
}

pub(super) struct WebGlGpuQueries;

impl GpuQueryBackend for WebGlGpuQueries {
    fn capability(&self) -> RenderGpuCapability {
        // SAFETY: Host returns a scalar for this context, no pointers or Rust reentry.
        match unsafe { gpu_query_capability() } {
            2 => RenderGpuCapability::Timestamps,
            1 => RenderGpuCapability::Elapsed,
            _ => RenderGpuCapability::Unsupported,
        }
    }

    fn timestamp_mask(&self) -> u64 {
        // SAFETY: Scalar context capability, retained nowhere by host.
        let bits = unsafe { gpu_query_timestamp_bits() };
        if (1..64).contains(&bits) {
            (1u64 << bits) - 1
        } else {
            u64::MAX
        }
    }

    fn create(&mut self) -> Option<u32> {
        // SAFETY: Bridge allocates context-owned query, passing no Rust pointers.
        let query = unsafe { gpu_query_create() };
        (query != 0).then_some(query)
    }

    fn delete(&mut self, query: u32) {
        // SAFETY: Pool invalidates the context-owned query before handle reuse; bridge retains no Rust reference.
        unsafe { gpu_query_delete(query) };
    }

    fn timestamp(&mut self, query: u32) {
        // SAFETY: Valid context-owned query and advertised timestamp capability; no pointers.
        unsafe { gpu_query_timestamp(query) };
    }

    fn begin_elapsed(&mut self, query: u32) {
        // SAFETY: Pool owns query and permits at most one active elapsed scope per context.
        unsafe { gpu_query_begin(query) };
    }

    fn end_elapsed(&mut self) {
        // SAFETY: Pool ends its single active context-owned elapsed query.
        unsafe { gpu_query_end() };
    }

    fn available(&mut self, query: u32) -> bool {
        // SAFETY: Ended context-owned query; scalar availability check does not wait for completion.
        unsafe { gpu_query_available(query) != 0 }
    }

    fn result(&mut self, query: u32) -> Option<u64> {
        // SAFETY: Pool checked completion first; host returns one copied scalar, no Rust pointers/reentry.
        let value = unsafe { gpu_query_result(query) };
        (value.is_finite()
            && (0.0..=9_007_199_254_740_991.0).contains(&value)
            && value.fract() == 0.0)
            .then_some(value as u64)
    }

    fn disjoint(&mut self) -> bool {
        // SAFETY: Scalar status of host-owned current context.
        unsafe { gpu_query_disjoint() != 0 }
    }

    fn context_lost(&mut self) -> bool {
        // SAFETY: Scalar lifecycle status, no pointer/reference retained.
        unsafe { gpu_query_context_lost() != 0 }
    }
}
