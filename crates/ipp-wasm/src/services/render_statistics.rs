//! The packed statistics record of `diagnostics` render builds.
//!
//! One export returns the whole record, so a capture reads every counter with
//! a single call instead of one export per counter. The worker mirrors the
//! layout in `packages/ipp-client/src/render-worker.ts` (`RECORD`).
//!
//! The record is [`RECORD_WORDS`] little-endian `u32` words at fixed offsets,
//! whatever capabilities are compiled in. Word [`FLAGS`] carries one bit per
//! compiled capability group ([`FLAG_SHADOWS`], [`FLAG_GUI`], [`FLAG_SURFACES`]);
//! words of an absent group are zero and must be read as unavailable. Per-frame
//! words describe the last completed render. `TOTAL_*` words accumulate that
//! counter over every completed render of this presentation service and
//! saturate; readers compare two captures to measure work between them.
//!
//! The GUI layout words come from the rendered World rather than the
//! renderer: `GUI_LAYOUT_REFLOWS` and `GUI_TEXT_MEASUREMENTS` count the work
//! of that World's latest layout pass, and their `TOTAL_*` words are the
//! World's own saturating totals, so comparing two captures of one World
//! measures the layout work between them.

use ipp_render_gl::RenderStatistics;

pub(crate) const FLAGS: usize = 0;
pub(crate) const UPLOADED_BYTES: usize = 1;
pub(crate) const TOTAL_UPLOADED_BYTES: usize = 2;
pub(crate) const UNSHADOWED_LIGHTS: usize = 3;
#[cfg_attr(
    not(feature = "shadows"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const SHADOW_DRAW_CALLS: usize = 4;
#[cfg_attr(
    not(feature = "shadows"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const SHADOW_RESIDENT_BYTES: usize = 5;
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const GUI_BATCHES: usize = 6;
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const GUI_REBUILDS: usize = 7;
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const GUI_ALLOCATIONS: usize = 8;
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const GUI_RESIDENT_BYTES: usize = 9;
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const GLYPH_MISSES: usize = 10;
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const GLYPH_POPULATES: usize = 11;
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const GLYPH_POPULATION_FAILURES: usize = 12;
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const GLYPH_PAGE_RETIREMENTS: usize = 13;
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const GLYPH_PAGES: usize = 14;
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const GLYPH_RESIDENT_BYTES: usize = 15;
pub(crate) const TOTAL_GUI_REBUILDS: usize = 16;
pub(crate) const TOTAL_GUI_ALLOCATIONS: usize = 17;
pub(crate) const TOTAL_GLYPH_MISSES: usize = 18;
pub(crate) const TOTAL_GLYPH_POPULATES: usize = 19;
pub(crate) const TOTAL_GLYPH_POPULATION_FAILURES: usize = 20;
pub(crate) const TOTAL_GLYPH_PAGE_RETIREMENTS: usize = 21;
#[cfg_attr(
    not(feature = "surfaces"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const ANALYTIC_GLYPH_RESIDENT_BYTES: usize = 22;
#[cfg_attr(
    not(feature = "surfaces"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const SURFACE_CACHE_REPAINTS: usize = 23;
#[cfg_attr(
    not(feature = "surfaces"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const SURFACE_CACHE_REUSES: usize = 24;
#[cfg_attr(
    not(feature = "surfaces"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const SURFACE_CACHE_DIRECT: usize = 25;
#[cfg_attr(
    not(feature = "surfaces"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const SURFACE_CACHE_FALLBACKS: usize = 26;
#[cfg_attr(
    not(feature = "surfaces"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const SURFACE_CACHE_ANIMATED: usize = 27;
#[cfg_attr(
    not(feature = "surfaces"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const SURFACE_CACHE_ALLOCATIONS: usize = 28;
#[cfg_attr(
    not(feature = "surfaces"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const SURFACE_CACHE_ENTRIES: usize = 29;
#[cfg_attr(
    not(feature = "surfaces"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const SURFACE_CACHE_RESIDENT_BYTES: usize = 30;
pub(crate) const TOTAL_SURFACE_CACHE_REPAINTS: usize = 31;
pub(crate) const TOTAL_SURFACE_CACHE_REUSES: usize = 32;
pub(crate) const TOTAL_SURFACE_CACHE_DIRECT: usize = 33;
pub(crate) const TOTAL_SURFACE_CACHE_FALLBACKS: usize = 34;
pub(crate) const TOTAL_SURFACE_CACHE_ALLOCATIONS: usize = 35;
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const GUI_LAYOUT_REFLOWS: usize = 36;
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const GUI_TEXT_MEASUREMENTS: usize = 37;
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const TOTAL_GUI_LAYOUT_REFLOWS: usize = 38;
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "zero word of a compiled-out capability")
)]
pub(crate) const TOTAL_GUI_TEXT_MEASUREMENTS: usize = 39;

/// Words in the record.
pub(crate) const RECORD_WORDS: usize = 40;

pub(crate) const FLAG_SHADOWS: u32 = 1;
pub(crate) const FLAG_GUI: u32 = 2;
pub(crate) const FLAG_SURFACES: u32 = 4;

/// Capability groups compiled into this runtime.
const FLAGS_VALUE: u32 = {
    let mut flags = 0;

    if cfg!(feature = "shadows") {
        flags |= FLAG_SHADOWS;
    }

    if cfg!(feature = "gui") {
        flags |= FLAG_GUI;
    }

    if cfg!(feature = "surfaces") {
        flags |= FLAG_SURFACES;
    }

    flags
};

/// Per-frame counters the record also accumulates.
const ACCUMULATED: [(usize, usize); 12] = [
    (UPLOADED_BYTES, TOTAL_UPLOADED_BYTES),
    (GUI_REBUILDS, TOTAL_GUI_REBUILDS),
    (GUI_ALLOCATIONS, TOTAL_GUI_ALLOCATIONS),
    (GLYPH_MISSES, TOTAL_GLYPH_MISSES),
    (GLYPH_POPULATES, TOTAL_GLYPH_POPULATES),
    (GLYPH_POPULATION_FAILURES, TOTAL_GLYPH_POPULATION_FAILURES),
    (GLYPH_PAGE_RETIREMENTS, TOTAL_GLYPH_PAGE_RETIREMENTS),
    (SURFACE_CACHE_REPAINTS, TOTAL_SURFACE_CACHE_REPAINTS),
    (SURFACE_CACHE_REUSES, TOTAL_SURFACE_CACHE_REUSES),
    (SURFACE_CACHE_DIRECT, TOTAL_SURFACE_CACHE_DIRECT),
    (SURFACE_CACHE_FALLBACKS, TOTAL_SURFACE_CACHE_FALLBACKS),
    (SURFACE_CACHE_ALLOCATIONS, TOTAL_SURFACE_CACHE_ALLOCATIONS),
];

/// Totals of the presentation service and the record storage it exports.
pub(crate) struct RenderStatisticsRecord {
    words: [u32; RECORD_WORDS],
    totals: [u32; RECORD_WORDS],
    /// Layout words of the World of the last completed render.
    #[cfg_attr(
        not(feature = "gui"),
        allow(dead_code, reason = "zero words of a compiled-out capability")
    )]
    layout: [u32; 4],
}

impl RenderStatisticsRecord {
    pub(crate) fn new() -> Self {
        Self {
            words: [0; RECORD_WORDS],
            totals: [0; RECORD_WORDS],
            layout: [0; 4],
        }
    }

    /// Keep the GUI layout counters of the World a completed render drew.
    #[cfg(feature = "gui")]
    pub(crate) fn record_layout(&mut self, statistics: ipp_core::GuiLayoutStatistics) {
        let word = |value: u64| u32::try_from(value).unwrap_or(u32::MAX);
        self.layout = [
            word(statistics.latest.reflows),
            word(statistics.latest.text_measurements),
            word(statistics.total.reflows),
            word(statistics.total.text_measurements),
        ];
    }

    /// Add one completed render to the totals: a few additions per frame, only
    /// in diagnostics builds.
    pub(crate) fn accumulate(&mut self, statistics: &RenderStatistics) {
        let frame = per_frame(statistics);

        for (word, total) in ACCUMULATED {
            self.totals[total] = self.totals[total].saturating_add(frame[word]);
        }
    }

    /// Fill the record from the last completed render and return its storage.
    pub(crate) fn fill(&mut self, statistics: &RenderStatistics) -> &[u32; RECORD_WORDS] {
        self.words = per_frame(statistics);
        self.words[FLAGS] = FLAGS_VALUE;

        for (_, total) in ACCUMULATED {
            self.words[total] = self.totals[total];
        }

        #[cfg(feature = "gui")]
        {
            [
                self.words[GUI_LAYOUT_REFLOWS],
                self.words[GUI_TEXT_MEASUREMENTS],
                self.words[TOTAL_GUI_LAYOUT_REFLOWS],
                self.words[TOTAL_GUI_TEXT_MEASUREMENTS],
            ] = self.layout;
        }

        &self.words
    }
}

/// Per-frame words of one render's statistics; totals and flags are zero.
fn per_frame(statistics: &RenderStatistics) -> [u32; RECORD_WORDS] {
    let mut words = [0; RECORD_WORDS];
    words[UPLOADED_BYTES] = statistics.uploaded_bytes;
    words[UNSHADOWED_LIGHTS] = statistics.unshadowed_lights;

    #[cfg(feature = "shadows")]
    {
        words[SHADOW_DRAW_CALLS] = statistics.shadow_draw_calls;
        words[SHADOW_RESIDENT_BYTES] = statistics.shadow_resident_bytes;
    }

    #[cfg(feature = "gui")]
    {
        words[GUI_BATCHES] = statistics.gui_batches;
        words[GUI_REBUILDS] = statistics.gui_rebuilds;
        words[GUI_ALLOCATIONS] = statistics.gui_allocations;
        words[GUI_RESIDENT_BYTES] = statistics.gui_resident_bytes;
        words[GLYPH_MISSES] = statistics.glyph_misses;
        words[GLYPH_POPULATES] = statistics.glyph_populates;
        words[GLYPH_POPULATION_FAILURES] = statistics.glyph_population_failures;
        words[GLYPH_PAGE_RETIREMENTS] = statistics.glyph_page_retirements;
        words[GLYPH_PAGES] = statistics.glyph_pages;
        words[GLYPH_RESIDENT_BYTES] =
            u32::try_from(statistics.glyph_resident_bytes).unwrap_or(u32::MAX);
    }

    #[cfg(feature = "surfaces")]
    {
        words[ANALYTIC_GLYPH_RESIDENT_BYTES] = statistics.analytic_glyph_resident_bytes;
        words[SURFACE_CACHE_REPAINTS] = statistics.surface_cache_repaints;
        words[SURFACE_CACHE_REUSES] = statistics.surface_cache_reuses;
        words[SURFACE_CACHE_DIRECT] = statistics.surface_cache_direct;
        words[SURFACE_CACHE_FALLBACKS] = statistics.surface_cache_fallbacks;
        words[SURFACE_CACHE_ANIMATED] = statistics.surface_cache_animated;
        words[SURFACE_CACHE_ALLOCATIONS] = statistics.surface_cache_allocations;
        words[SURFACE_CACHE_ENTRIES] = statistics.surface_cache_entries;
        words[SURFACE_CACHE_RESIDENT_BYTES] = statistics.surface_cache_resident_bytes;
    }

    words
}

#[cfg(test)]
#[path = "render_statistics_tests.rs"]
mod tests;
