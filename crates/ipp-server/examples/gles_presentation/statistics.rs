//! JSON renderer observations for `RenderStatisticsSnapshot`, separate from capture stamps.
//!
//! The groups and names follow `packages/ipp-client/src/presentation.ts`, which
//! the worker fills from the packed WASM record: per-frame counters describe the latest completed draw
//! and `total*` counters accumulate over every render of this presentation.
//! Identities that JavaScript reads as `bigint` are decimal strings.

use std::fmt::Write;

use ipp_core::WorldId;
use ipp_render_gl::{GlesRenderDevice, RenderService, RenderStatistics};

/// Running totals of the per-frame counters, saturating like the WASM record.
#[derive(Default)]
pub(super) struct StatisticsTotals {
    uploaded_bytes: u32,
    gui: [u32; 6],
    layout: ipp_host_session::services::gui_layout_statistics::HostGuiLayoutStatistics,
    surface_cache: [u32; 5],
}

impl StatisticsTotals {
    /// Add one completed render: a few additions per frame in this test host.
    pub(super) fn accumulate(&mut self, statistics: &RenderStatistics) {
        self.uploaded_bytes = self
            .uploaded_bytes
            .saturating_add(statistics.uploaded_bytes);

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

    pub(super) fn record_frame(
        &mut self,
        host: &mut ipp_core::HostRuntime,
        frame: &ipp_core::HostFrameReport,
    ) {
        self.layout.record(host, frame);
    }
}

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

    write!(
        json,
        ",\"shadows\":{{\"shadowDrawCalls\":{},\"shadowResidentBytes\":{}}}",
        statistics.shadow_draw_calls, statistics.shadow_resident_bytes
    )
    .expect("string write");

    let [
        total_rebuilds,
        total_allocations,
        total_misses,
        total_populates,
        total_failures,
        total_retirements,
    ] = totals.gui;
    write!(
        json,
        ",\"gui\":{{\"guiBatches\":{},\"guiRebuilds\":{},\"guiAllocations\":{},\"guiResidentBytes\":{},\"glyphMisses\":{},\"glyphPopulates\":{},\"glyphPopulationFailures\":{},\"glyphPageRetirements\":{},\"glyphPages\":{},\"glyphResidentBytes\":{},\"totalGuiRebuilds\":{total_rebuilds},\"totalGuiAllocations\":{total_allocations},\"totalGlyphMisses\":{total_misses},\"totalGlyphPopulates\":{total_populates},\"totalGlyphPopulationFailures\":{total_failures},\"totalGlyphPageRetirements\":{total_retirements}",
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

    if let Some([reflows, measurements, total_reflows, total_measurements]) = totals.layout.words()
    {
        write!(json, ",\"guiLayoutReflows\":{reflows},\"guiTextMeasurements\":{measurements},\"totalGuiLayoutReflows\":{total_reflows},\"totalGuiTextMeasurements\":{total_measurements}").expect("string write");
    }
    json.push('}');

    json.push_str(",\"guiLayout\":");
    totals.layout.write_json(&mut json);

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
