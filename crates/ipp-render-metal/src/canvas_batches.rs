//! GPU draw records retained between frames for unchanged ordered Canvas runs.

use std::{
    collections::BTreeMap,
    ops::Range,
    sync::Arc,
    time::{Duration, Instant},
};

use ipp_core::{
    OutputRef,
    systems::canvas::{CanvasPaintEntry, CanvasPublication},
};

use crate::{
    analytic_surface::AnalyticMapping,
    canvas::CanvasProjection,
    device::{Frame, Object},
    service::MetalFrameSummary,
};

const MAX_RETAINED_BYTES: usize = 24 * 1024 * 1024;
const MAX_RUNS: usize = 256;
// Let streaming geometry settle before creating long-lived uploads.
const STABLE_FOR: Duration = Duration::from_millis(750);
pub(super) const PRIMITIVES_PER_RUN: usize = 64;

#[derive(Clone, Copy, PartialEq)]
enum Projection {
    Analytic(AnalyticMapping, [f64; 2], [f64; 2]),
    Plane {
        matrix: [f32; 16],
        world: Option<[f32; 16]>,
    },
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct RunKey {
    projection: Projection,
    viewport: [u32; 2],
    extent: [f32; 2],
    resources: u64,
    paints: u64,
}

impl RunKey {
    pub(super) fn new(
        projection: &CanvasProjection<'_>,
        viewport: [u32; 2],
        canvas: &CanvasPublication,
    ) -> Option<Self> {
        let projection = match projection {
            CanvasProjection::Analytic(surface) => {
                let (mapping, scale, extent) = surface.batch_mapping();
                Projection::Analytic(mapping, scale, extent)
            }
            CanvasProjection::Plane {
                matrix,
                world,
            } => Projection::Plane {
                matrix: *matrix,
                world: *world,
            },
            CanvasProjection::Surface {
                ..
            } => return None,
        };
        Some(Self {
            projection,
            viewport,
            extent: canvas.logical_extent,
            resources: canvas.resource_revision,
            paints: canvas.paints_revision,
        })
    }
}

struct Run {
    key: RunKey,
    entries: Arc<[Arc<CanvasPaintEntry>]>,
    range: Range<usize>,
    last_frame: u64,
    record_after: Instant,
    recording: Option<Recording>,
}

impl Run {
    fn matches(
        &self,
        key: RunKey,
        entries: &Arc<[Arc<CanvasPaintEntry>]>,
        range: &Range<usize>,
    ) -> bool {
        self.key == key
            && self.range == *range
            && (Arc::ptr_eq(&self.entries, entries)
                || self.entries[self.range.clone()]
                    .iter()
                    .zip(&entries[range.clone()])
                    .all(|(old, new)| Arc::ptr_eq(old, new)))
    }
}

struct Recording {
    buffer: Object,
    bytes: usize,
    draw_calls: u32,
    triangles: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct RunId {
    output: OutputRef,
    start: usize,
    near_root: bool,
}

pub(super) struct CanvasBatches {
    runs: BTreeMap<RunId, Run>,
    bytes: usize,
    frame: u64,
    now: Instant,
}

impl Default for CanvasBatches {
    fn default() -> Self {
        Self {
            runs: BTreeMap::new(),
            bytes: 0,
            frame: 0,
            now: Instant::now(),
        }
    }
}

impl CanvasBatches {
    pub(super) fn begin_frame(&mut self, frame: u64) {
        self.frame = frame;
        self.now = Instant::now();
        if frame.is_multiple_of(32) {
            let bytes = &mut self.bytes;
            self.runs.retain(|_, run| {
                let keep = frame - run.last_frame <= 120;
                if !keep && let Some(recording) = &run.recording {
                    *bytes -= recording.bytes;
                }
                keep
            });
        }
    }

    pub(super) fn lookup(
        &mut self,
        canvas: &CanvasPublication,
        key: RunKey,
        range: Range<usize>,
        frame: &mut Frame,
        summary: &mut MetalFrameSummary,
    ) -> Lookup {
        // Both intersections can be submitted in one frame. Let each pass
        // retain its own ordered draws while the full key fences view changes.
        let id = RunId {
            output: canvas.selection,
            start: range.start,
            near_root: matches!(key.projection, Projection::Analytic(mapping, ..) if mapping.is_near_root()),
        };
        if let Some(run) = self.runs.get_mut(&id)
            && run.matches(key, &canvas.entries, &range)
        {
            run.last_frame = self.frame;
            if !Arc::ptr_eq(&run.entries, &canvas.entries) {
                run.entries = canvas.entries.clone();
            }
            if let Some(recording) = &run.recording {
                #[cfg(feature = "instrumentation")]
                let _trace = ipp_core::profiling::CpuTraceScope::new("MetalCanvasBatches.replay");
                if frame.replay_canvas(&recording.buffer) {
                    summary.draw_calls = summary.draw_calls.saturating_add(recording.draw_calls);
                    summary.triangles = summary.triangles.saturating_add(recording.triangles);
                } else {
                    summary.fail();
                }
                return Lookup::Replayed;
            }
            if self.now >= run.record_after
                && self.bytes < MAX_RETAINED_BYTES
                && frame.begin_canvas_record()
            {
                return Lookup::Record(id);
            }
            return Lookup::Direct;
        }

        if let Some(old) = self.runs.remove(&id).and_then(|run| run.recording) {
            self.bytes -= old.bytes;
        }
        if self.runs.len() < MAX_RUNS {
            self.runs.insert(
                id,
                Run {
                    key,
                    entries: canvas.entries.clone(),
                    range,
                    last_frame: self.frame,
                    record_after: self.now + STABLE_FOR,
                    recording: None,
                },
            );
        }
        Lookup::Direct
    }

    pub(super) fn finish(&mut self, id: RunId, frame: &mut Frame, counts: Option<(u32, u32)>) {
        let Some((buffer, bytes)) = frame.end_canvas_record() else {
            return;
        };
        let Some((draw_calls, triangles)) = counts else {
            return;
        };
        if self.bytes + bytes <= MAX_RETAINED_BYTES
            && let Some(run) = self.runs.get_mut(&id)
        {
            self.bytes += bytes;
            run.recording = Some(Recording {
                buffer,
                bytes,
                draw_calls,
                triangles,
            });
        }
    }
}

pub(super) enum Lookup {
    Replayed,
    Record(RunId),
    Direct,
}
