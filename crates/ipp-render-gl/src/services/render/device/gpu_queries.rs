//! Instrumentation-only, context-owned asynchronous timer queries.
//! Limits bound diagnostic queries, never Worlds, rendering or artifact size.

/// Optional timer capability of this graphics context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderGpuCapability {
    /// The context cannot provide valid timer data.
    Unsupported,
    /// Only one nonoverlapping elapsed scope can be active.
    Elapsed,
    /// Timestamp pairs permit overlapping/nested scopes.
    Timestamps,
}

/// Query completion distinguishes unavailable data from measured zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderGpuAvailability {
    /// Issued commands have not completed asynchronously.
    Pending,
    /// The ended query completed with a valid GPU interval.
    Available {
        /// GPU command duration in nanoseconds.
        duration_ns: u64,
    },
    /// The context cannot provide valid timer data.
    Unsupported,
    /// A clock discontinuity invalidated pending samples.
    Disjoint,
    /// The issuing graphics context was lost.
    ContextLost,
    /// Diagnostic query storage is full; rendering continues.
    CapacityDropped,
    /// Elapsed timing cannot nest within an active interval.
    OverlapSkipped,
    /// Capture stopped before a valid result was available.
    Stopped,
}

/// Opaque token unique within one device lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderGpuQueryToken(u64);

/// Physical queries per context; timestamp scopes consume two each.
pub const GPU_QUERY_CAPACITY: usize = 256;

pub(super) trait GpuQueryBackend {
    fn capability(&self) -> RenderGpuCapability;
    fn timestamp_mask(&self) -> u64 {
        u64::MAX
    }
    fn create(&mut self) -> Option<u32>;
    fn delete(&mut self, query: u32);
    fn timestamp(&mut self, query: u32);
    fn begin_elapsed(&mut self, query: u32);
    fn end_elapsed(&mut self);
    fn available(&mut self, query: u32) -> bool;
    fn result(&mut self, query: u32) -> Option<u64>;
    fn disjoint(&mut self) -> bool;
    fn context_lost(&mut self) -> bool;
}

struct Query {
    token: RenderGpuQueryToken,
    first: u32,
    last: Option<u32>,
    ended: bool,
    terminal: Option<RenderGpuAvailability>,
}

pub(super) struct GpuQueryPool<B: GpuQueryBackend> {
    backend: B,
    queries: Vec<Query>,
    next_token: u64,
    active_elapsed: Option<RenderGpuQueryToken>,
    capacity: usize,
}

impl<B: GpuQueryBackend> GpuQueryPool<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            queries: Vec::new(),
            next_token: 0,
            active_elapsed: None,
            capacity: GPU_QUERY_CAPACITY,
        }
    }

    pub fn capability(&self) -> RenderGpuCapability {
        self.backend.capability()
    }

    pub fn start(&mut self) -> Result<RenderGpuQueryToken, RenderGpuAvailability> {
        if self.backend.context_lost() {
            self.stop(RenderGpuAvailability::ContextLost);
            return Err(RenderGpuAvailability::ContextLost);
        }
        let capability = self.capability();
        if capability == RenderGpuCapability::Unsupported {
            return Err(RenderGpuAvailability::Unsupported);
        }
        if self.active_elapsed.is_some() {
            return Err(RenderGpuAvailability::OverlapSkipped);
        }
        if self.queries.len() >= self.capacity / 2 {
            return Err(RenderGpuAvailability::CapacityDropped);
        }
        if self.backend.disjoint() {
            self.stop(RenderGpuAvailability::Disjoint);
        }
        let next_token = self
            .next_token
            .checked_add(1)
            .ok_or(RenderGpuAvailability::CapacityDropped)?;

        let first = self
            .backend
            .create()
            .ok_or(RenderGpuAvailability::CapacityDropped)?;
        let last = if capability == RenderGpuCapability::Timestamps {
            match self.backend.create() {
                Some(query) => Some(query),
                None => {
                    self.backend.delete(first);
                    return Err(RenderGpuAvailability::CapacityDropped);
                }
            }
        } else {
            None
        };
        self.next_token = next_token;
        let token = RenderGpuQueryToken(self.next_token);
        if last.is_some() {
            self.backend.timestamp(first);
        } else {
            self.backend.begin_elapsed(first);
            self.active_elapsed = Some(token);
        }
        self.queries.push(Query {
            token,
            first,
            last,
            ended: false,
            terminal: None,
        });
        Ok(token)
    }

    pub fn end(&mut self, token: RenderGpuQueryToken) {
        if self.backend.context_lost() {
            self.stop(RenderGpuAvailability::ContextLost);
            return;
        }
        let Some(query) = self.queries.iter_mut().find(|query| query.token == token) else {
            return;
        };
        if query.ended || query.terminal.is_some() {
            return;
        }
        if let Some(last) = query.last {
            self.backend.timestamp(last);
        } else {
            self.backend.end_elapsed();
            self.active_elapsed = None;
        }
        query.ended = true;
    }

    pub fn poll(&mut self, token: RenderGpuQueryToken) -> RenderGpuAvailability {
        if self.backend.context_lost() {
            self.stop(RenderGpuAvailability::ContextLost);
        } else if self.capability() != RenderGpuCapability::Unsupported && self.backend.disjoint() {
            self.stop(RenderGpuAvailability::Disjoint);
        }
        let Some(index) = self.queries.iter().position(|query| query.token == token) else {
            return RenderGpuAvailability::Stopped;
        };
        let query = &self.queries[index];
        let result = if let Some(terminal) = query.terminal {
            terminal
        } else if !query.ended
            || !self.backend.available(query.first)
            || query.last.is_some_and(|last| !self.backend.available(last))
        {
            return RenderGpuAvailability::Pending;
        } else {
            let first = self.backend.result(query.first);
            let duration = match query.last {
                Some(last) => first
                    .zip(self.backend.result(last))
                    .map(|(first, last)| last.wrapping_sub(first) & self.backend.timestamp_mask()),
                None => first,
            };
            duration.map_or(RenderGpuAvailability::Unsupported, |duration_ns| {
                RenderGpuAvailability::Available {
                    duration_ns,
                }
            })
        };
        let query = self.queries.remove(index);
        if query.terminal.is_none() {
            self.backend.delete(query.first);
            if let Some(last) = query.last {
                self.backend.delete(last);
            }
        }
        result
    }

    pub fn stop(&mut self, reason: RenderGpuAvailability) {
        let reason = match reason {
            RenderGpuAvailability::Disjoint | RenderGpuAvailability::ContextLost => reason,
            _ => RenderGpuAvailability::Stopped,
        };
        let lost = reason == RenderGpuAvailability::ContextLost || self.backend.context_lost();
        if self.active_elapsed.take().is_some() && !lost {
            self.backend.end_elapsed();
        }
        for query in &mut self.queries {
            if query.terminal.is_some() {
                continue;
            }
            if !lost {
                self.backend.delete(query.first);
                if let Some(last) = query.last {
                    self.backend.delete(last);
                }
            }
            query.terminal = Some(if lost {
                RenderGpuAvailability::ContextLost
            } else {
                reason
            });
        }
    }
}

impl<B: GpuQueryBackend> Drop for GpuQueryPool<B> {
    fn drop(&mut self) {
        self.stop(RenderGpuAvailability::Stopped);
    }
}

#[path = "gpu_query_tests.rs"]
#[cfg(test)]
mod tests;

#[cfg(target_arch = "wasm32")]
impl Default for GpuQueryPool<super::webgl_gpu_queries::WebGlGpuQueries> {
    fn default() -> Self {
        Self::new(super::webgl_gpu_queries::WebGlGpuQueries)
    }
}
