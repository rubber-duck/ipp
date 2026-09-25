//! JSON frame summaries and the capture statistics of `RenderStatisticsSnapshot`.
//!
//! The groups and names follow `packages/ipp-client/src/presentation.ts`, which
//! the worker fills from the packed WASM record: a group is absent when its
//! capability is compiled out, per-frame counters describe the captured frame
//! and `total*` counters accumulate over every render of this presentation.
//! Identities that JavaScript reads as `bigint` are decimal strings.

use std::fmt::Write;

use ipp_core::WorldId;
use ipp_render_gl::{GlesRenderDevice, RenderFrameSummary, RenderService, RenderStatistics};

/// Summary of a completed frame, with statistics only for a capture.
pub(super) struct FrameHeader {
    pub(super) session: u64,
    pub(super) tick: u64,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) summary: RenderFrameSummary,
    pub(super) context_generation: u32,
    pub(super) statistics: Option<String>,
}

impl FrameHeader {
    pub(super) fn to_json(&self) -> String {
        let summary = &self.summary;
        let mut json = format!(
            "{{\"session\":\"{}\",\"tick\":\"{}\",\"width\":{},\"height\":{},\"drawCalls\":{},\"triangles\":{},\"failedDrawCalls\":{},\"invalidCamera\":{},\"contextGeneration\":{}",
            self.session,
            self.tick,
            self.width,
            self.height,
            summary.draw_calls,
            summary.triangles,
            summary.failed_draw_calls,
            summary.invalid_camera,
            self.context_generation
        );
        if let Some(statistics) = &self.statistics {
            write!(json, ",\"statistics\":{statistics}").expect("string write");
        }

        json.push('}');
        json
    }
}

/// Running totals of the per-frame counters, saturating like the WASM record.
#[derive(Default)]
pub(super) struct StatisticsTotals {
    uploaded_bytes: u32,
    #[cfg(feature = "gui")]
    gui: [u32; 6],
    /// Latest-pass and total layout counters of the last rendered World.
    #[cfg(feature = "gui")]
    layout: [u32; 4],
    surface_cache: [u32; 5],
}

impl StatisticsTotals {
    /// Add one completed render: a few additions per frame in this test host.
    pub(super) fn accumulate(&mut self, statistics: &RenderStatistics) {
        self.uploaded_bytes = self
            .uploaded_bytes
            .saturating_add(statistics.uploaded_bytes);

        #[cfg(feature = "gui")]
        for (total, frame) in self.gui.iter_mut().zip(gui_accumulated(statistics)) {
            *total = total.saturating_add(frame);
        }

        for (total, frame) in self
            .surface_cache
            .iter_mut()
            .zip(surface_cache_accumulated(statistics))
        {
            *total = total.saturating_add(frame);
        }
    }

    /// Keep the GUI layout counters of the World a completed render drew.
    #[cfg(feature = "gui")]
    pub(super) fn record_layout(&mut self, statistics: ipp_core::GuiLayoutStatistics) {
        let word = |value: u64| u32::try_from(value).unwrap_or(u32::MAX);
        self.layout = [
            word(statistics.latest.reflows),
            word(statistics.latest.text_measurements),
            word(statistics.total.reflows),
            word(statistics.total.text_measurements),
        ];
    }
}

#[cfg(feature = "gui")]
fn gui_accumulated(statistics: &RenderStatistics) -> [u32; 6] {
    [
        statistics.gui_rebuilds,
        statistics.gui_allocations,
        statistics.glyph_misses,
        statistics.glyph_populates,
        statistics.glyph_population_failures,
        statistics.glyph_page_retirements,
    ]
}

fn surface_cache_accumulated(statistics: &RenderStatistics) -> [u32; 5] {
    [
        statistics.surface_cache_repaints,
        statistics.surface_cache_reuses,
        statistics.surface_cache_direct,
        statistics.surface_cache_fallbacks,
        statistics.surface_cache_allocations,
    ]
}

/// The statistics object of a capture of `world`'s last completed render.
pub(super) fn snapshot(
    renderer: &mut RenderService<GlesRenderDevice>,
    world: WorldId,
    totals: &StatisticsTotals,
    readback_ms: f64,
    device: &[String; 3],
) -> String {
    let statistics = *renderer.statistics();
    let mut json = format!(
        "{{\"readbackMs\":{readback_ms},\"frame\":{{\"uploadedBytes\":{},\"totalUploadedBytes\":{},\"unshadowedLights\":{}}}",
        statistics.uploaded_bytes, totals.uploaded_bytes, statistics.unshadowed_lights
    );

    #[cfg(feature = "shadows")]
    write!(
        json,
        ",\"shadows\":{{\"shadowDrawCalls\":{},\"shadowResidentBytes\":{}}}",
        statistics.shadow_draw_calls, statistics.shadow_resident_bytes
    )
    .expect("string write");

    #[cfg(feature = "gui")]
    {
        let [
            total_rebuilds,
            total_allocations,
            total_misses,
            total_populates,
            total_failures,
            total_retirements,
        ] = totals.gui;
        let [reflows, measurements, total_reflows, total_measurements] = totals.layout;
        write!(
            json,
            ",\"gui\":{{\"guiBatches\":{},\"guiRebuilds\":{},\"guiAllocations\":{},\"guiResidentBytes\":{},\"glyphMisses\":{},\"glyphPopulates\":{},\"glyphPopulationFailures\":{},\"glyphPageRetirements\":{},\"glyphPages\":{},\"glyphResidentBytes\":{},\"totalGuiRebuilds\":{total_rebuilds},\"totalGuiAllocations\":{total_allocations},\"totalGlyphMisses\":{total_misses},\"totalGlyphPopulates\":{total_populates},\"totalGlyphPopulationFailures\":{total_failures},\"totalGlyphPageRetirements\":{total_retirements},\"guiLayoutReflows\":{reflows},\"guiTextMeasurements\":{measurements},\"totalGuiLayoutReflows\":{total_reflows},\"totalGuiTextMeasurements\":{total_measurements}}}",
            statistics.gui_batches,
            statistics.gui_rebuilds,
            statistics.gui_allocations,
            statistics.gui_resident_bytes,
            statistics.glyph_misses,
            statistics.glyph_populates,
            statistics.glyph_population_failures,
            statistics.glyph_page_retirements,
            statistics.glyph_pages,
            u32::try_from(statistics.glyph_resident_bytes).unwrap_or(u32::MAX),
        )
        .expect("string write");
    }

    let [
        total_repaints,
        total_reuses,
        total_direct,
        total_fallbacks,
        total_allocations,
    ] = totals.surface_cache;
    write!(
        json,
        ",\"surfaces\":{{\"analyticGlyphResidentBytes\":{},\"surfaceCacheRepaints\":{},\"surfaceCacheReuses\":{},\"surfaceCacheDirect\":{},\"surfaceCacheFallbacks\":{},\"surfaceCacheAnimated\":{},\"surfaceCacheAllocations\":{},\"surfaceCacheEntries\":{},\"surfaceCacheResidentBytes\":{},\"totalSurfaceCacheRepaints\":{total_repaints},\"totalSurfaceCacheReuses\":{total_reuses},\"totalSurfaceCacheDirect\":{total_direct},\"totalSurfaceCacheFallbacks\":{total_fallbacks},\"totalSurfaceCacheAllocations\":{total_allocations},\"surfaceCaches\":[",
        statistics.analytic_glyph_resident_bytes,
        statistics.surface_cache_repaints,
        statistics.surface_cache_reuses,
        statistics.surface_cache_direct,
        statistics.surface_cache_fallbacks,
        statistics.surface_cache_animated,
        statistics.surface_cache_allocations,
        statistics.surface_cache_entries,
        statistics.surface_cache_resident_bytes,
    )
    .expect("string write");

    let mut records = Vec::new();
    renderer.surface_cache_diagnostics(world, &mut records);
    for (index, record) in records.iter().enumerate() {
        let painted_at_ms = (record.painted_at * 1_000.0)
            .round()
            .clamp(0.0, f64::from(u32::MAX)) as u32;
        write!(
            json,
            "{}{{\"entity\":\"{}\",\"mode\":{},\"band\":{},\"width\":{},\"height\":{},\"repaints\":{},\"reuses\":{},\"paintedAtMs\":{painted_at_ms},\"residentBytes\":{}}}",
            if index == 0 { "" } else { "," },
            record.entity.to_bits(),
            record.presentation.code(),
            record.band,
            record.size[0],
            record.size[1],
            record.repaints,
            record.reuses,
            record.resident_bytes,
        )
        .expect("string write");
    }

    let [version, _vendor, renderer_name] = device;
    write!(
        json,
        "]}},\"device\":{{\"api\":\"OpenGL ES\",\"version\":{},\"renderer\":{}}}}}",
        quoted(version),
        quoted(renderer_name)
    )
    .expect("string write");
    json
}

/// A JSON string literal, escaping quotes, backslashes and control characters.
fn quoted(value: &str) -> String {
    let mut json = String::with_capacity(value.len() + 2);
    json.push('"');
    for character in value.chars() {
        match character {
            '"' => json.push_str("\\\""),
            '\\' => json.push_str("\\\\"),
            character if character.is_control() => {
                write!(json, "\\u{:04x}", u32::from(character)).expect("string write")
            }
            character => json.push(character),
        }
    }

    json.push('"');
    json
}
