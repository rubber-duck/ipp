//! Optional EXT timer entry points; absence never fails ordinary device setup.
use super::super::gpu_queries::GpuQueryBackend;
use super::super::{RenderGpuAvailability, RenderGpuCapability, RenderGpuQueryToken};
use super::*;

const ELAPSED: u32 = 0x88BF;
const TIMESTAMP: u32 = 0x8E28;
const DISJOINT: u32 = 0x8FBB;

struct TimerFunctions {
    generate: unsafe extern "system" fn(i32, *mut u32),
    delete: unsafe extern "system" fn(i32, *const u32),
    begin: unsafe extern "system" fn(u32, u32),
    end: unsafe extern "system" fn(u32),
    query: unsafe extern "system" fn(u32, u32, *mut i32),
    available: unsafe extern "system" fn(u32, u32, *mut u32),
    result: unsafe extern "system" fn(u32, u32, *mut u64),
    timestamp: Option<unsafe extern "system" fn(u32, u32)>,
}

pub(super) struct GlesGpuQueries {
    calls: super::super::gl_call_counts::GlCallCounters,
    functions: Option<TimerFunctions>,
    get_integer: unsafe extern "system" fn(u32, *mut i32),
    reset: Option<unsafe extern "system" fn() -> u32>,
    capability: RenderGpuCapability,
    timestamp_mask: u64,
}

impl GlesGpuQueries {
    /// # Safety
    /// The caller keeps the exact-signature loader and current context valid through device drop.
    pub(super) unsafe fn load(
        gl: &Functions,
        reset: Option<unsafe extern "system" fn() -> u32>,
        loader: &mut impl FnMut(&CStr) -> *const c_void,
    ) -> Self {
        let mut result = Self {
            calls: gl.calls.clone(),
            functions: None,
            get_integer: gl.get_integer,
            reset,
            capability: RenderGpuCapability::Unsupported,
            timestamp_mask: u64::MAX,
        };
        // SAFETY: Current GLES context owns this terminated string; read-only borrow ends here.
        let extensions = unsafe { gl.get_string(0x1F03) };
        if extensions.is_null() {
            return result;
        }
        // SAFETY: Non-null terminated context-owned string, never retained after device lifetime.
        let extensions = unsafe { CStr::from_ptr(extensions.cast()) }.to_string_lossy();
        if !extensions
            .split_whitespace()
            .any(|name| name == "GL_EXT_disjoint_timer_query")
        {
            return result;
        }
        macro_rules! required {
            ($name:literal, $ty:ty) => {{
                let pointer = loader($name);
                if pointer.is_null() {
                    return result;
                }
                // SAFETY: Loader contract supplies this exact EXT signature and context lifetime.
                unsafe { std::mem::transmute::<*const c_void, $ty>(pointer) }
            }};
        }
        let timestamp = loader(c"glQueryCounterEXT");
        let functions = TimerFunctions {
            generate: required!(c"glGenQueriesEXT", unsafe extern "system" fn(i32, *mut u32)),
            delete: required!(
                c"glDeleteQueriesEXT",
                unsafe extern "system" fn(i32, *const u32)
            ),
            begin: required!(c"glBeginQueryEXT", unsafe extern "system" fn(u32, u32)),
            end: required!(c"glEndQueryEXT", unsafe extern "system" fn(u32)),
            query: required!(
                c"glGetQueryivEXT",
                unsafe extern "system" fn(u32, u32, *mut i32)
            ),
            available: required!(
                c"glGetQueryObjectuivEXT",
                unsafe extern "system" fn(u32, u32, *mut u32)
            ),
            result: required!(
                c"glGetQueryObjectui64vEXT",
                unsafe extern "system" fn(u32, u32, *mut u64)
            ),
            timestamp: if timestamp.is_null() {
                None
            } else {
                // SAFETY: Optional non-null loader result has exact EXT signature and same lifetime.
                Some(unsafe {
                    std::mem::transmute::<*const c_void, unsafe extern "system" fn(u32, u32)>(
                        timestamp,
                    )
                })
            },
        };
        let mut elapsed_bits = 0;
        let mut timestamp_bits = 0;
        // SAFETY: Current context writes one scalar each into exclusive live locals.
        unsafe {
            (functions.query)(ELAPSED, 0x8864, &mut elapsed_bits);
            (functions.query)(TIMESTAMP, 0x8864, &mut timestamp_bits);
        }
        result.capability = if timestamp_bits > 0 && functions.timestamp.is_some() {
            RenderGpuCapability::Timestamps
        } else if elapsed_bits > 0 {
            RenderGpuCapability::Elapsed
        } else {
            RenderGpuCapability::Unsupported
        };
        result.timestamp_mask = if (1..64).contains(&timestamp_bits) {
            (1u64 << timestamp_bits) - 1
        } else {
            u64::MAX
        };
        result.functions = Some(functions);
        result
    }
}

impl GpuQueryBackend for GlesGpuQueries {
    fn capability(&self) -> RenderGpuCapability {
        self.capability
    }

    fn timestamp_mask(&self) -> u64 {
        self.timestamp_mask
    }

    fn create(&mut self) -> Option<u32> {
        let functions = self.functions.as_ref()?;
        let mut query = 0;
        self.calls
            .record(super::super::gl_call_counts::GlCallCategory::Profiler);
        // SAFETY: Current context writes one query name to exclusive local; device owns deletion.
        unsafe { (functions.generate)(1, &mut query) };
        (query != 0).then_some(query)
    }

    fn delete(&mut self, query: u32) {
        self.calls
            .record(super::super::gl_call_counts::GlCallCategory::Profiler);
        // SAFETY: Query belongs to current context and is invalidated before reuse; no retained pointer.
        unsafe { (self.functions.as_ref().unwrap().delete)(1, &query) };
    }

    fn timestamp(&mut self, query: u32) {
        self.calls
            .record(super::super::gl_call_counts::GlCallCategory::Profiler);
        // SAFETY: Capability validated entry point; query owned by current context, no Rust pointer.
        unsafe { (self.functions.as_ref().unwrap().timestamp.unwrap())(query, TIMESTAMP) };
    }

    fn begin_elapsed(&mut self, query: u32) {
        self.calls
            .record(super::super::gl_call_counts::GlCallCategory::Profiler);
        // SAFETY: Pool guarantees no active elapsed query and owns context-local name.
        unsafe { (self.functions.as_ref().unwrap().begin)(ELAPSED, query) };
    }

    fn end_elapsed(&mut self) {
        self.calls
            .record(super::super::gl_call_counts::GlCallCategory::Profiler);
        // SAFETY: Pool ends the single active elapsed query in the current context.
        unsafe { (self.functions.as_ref().unwrap().end)(ELAPSED) };
    }

    fn available(&mut self, query: u32) -> bool {
        self.calls
            .record(super::super::gl_call_counts::GlCallCategory::Profiler);
        let mut available = 0;
        // SAFETY: Ended context-local query; availability read writes one exclusive scalar without waiting.
        unsafe { (self.functions.as_ref().unwrap().available)(query, 0x8867, &mut available) };
        available != 0
    }

    fn result(&mut self, query: u32) -> Option<u64> {
        self.calls
            .record(super::super::gl_call_counts::GlCallCategory::Profiler);
        let mut result = 0;
        // SAFETY: Pool checked availability for this ended query; read writes one exclusive u64 and cannot block for GPU completion.
        unsafe { (self.functions.as_ref().unwrap().result)(query, 0x8866, &mut result) };
        Some(result)
    }

    fn disjoint(&mut self) -> bool {
        if self.capability == RenderGpuCapability::Unsupported {
            return false;
        }
        self.calls
            .record(super::super::gl_call_counts::GlCallCategory::Profiler);
        let mut disjoint = 0;
        // SAFETY: Extension verified; current context writes one exclusive scalar, retained nowhere.
        unsafe { (self.get_integer)(DISJOINT, &mut disjoint) };
        disjoint != 0
    }

    fn context_lost(&mut self) -> bool {
        self.reset.is_some_and(|reset| {
            self.calls
                .record(super::super::gl_call_counts::GlCallCategory::Profiler);
            // SAFETY: Optional reset-status entry point belongs to device's current context lifetime.
            unsafe { reset() != 0 }
        })
    }
}

impl GlesRenderDevice {
    pub(super) fn gpu_start_query(&mut self) -> Result<RenderGpuQueryToken, RenderGpuAvailability> {
        self.gpu_queries.start()
    }

    pub(super) fn gpu_end_query(&mut self, token: RenderGpuQueryToken) {
        self.gpu_queries.end(token);
    }

    pub(super) fn gpu_poll_query(&mut self, token: RenderGpuQueryToken) -> RenderGpuAvailability {
        self.gpu_queries.poll(token)
    }

    pub(super) fn gpu_stop_queries(&mut self, reason: RenderGpuAvailability) {
        self.gpu_queries.stop(reason);
    }
}
