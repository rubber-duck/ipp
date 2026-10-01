use super::*;

/// Every named word, in layout order, as the worker reader names it.
const NAMED_WORDS: [(&str, usize); RECORD_WORDS] = [
    ("flags", FLAGS),
    ("uploadedBytes", UPLOADED_BYTES),
    ("totalUploadedBytes", TOTAL_UPLOADED_BYTES),
    ("unshadowedLights", UNSHADOWED_LIGHTS),
    ("shadowDrawCalls", SHADOW_DRAW_CALLS),
    ("shadowResidentBytes", SHADOW_RESIDENT_BYTES),
    ("guiBatches", GUI_BATCHES),
    ("guiRebuilds", GUI_REBUILDS),
    ("guiAllocations", GUI_ALLOCATIONS),
    ("guiResidentBytes", GUI_RESIDENT_BYTES),
    ("glyphMisses", GLYPH_MISSES),
    ("glyphPopulates", GLYPH_POPULATES),
    ("glyphPopulationFailures", GLYPH_POPULATION_FAILURES),
    ("glyphPageRetirements", GLYPH_PAGE_RETIREMENTS),
    ("glyphPages", GLYPH_PAGES),
    ("glyphResidentBytes", GLYPH_RESIDENT_BYTES),
    ("totalGuiRebuilds", TOTAL_GUI_REBUILDS),
    ("totalGuiAllocations", TOTAL_GUI_ALLOCATIONS),
    ("totalGlyphMisses", TOTAL_GLYPH_MISSES),
    ("totalGlyphPopulates", TOTAL_GLYPH_POPULATES),
    (
        "totalGlyphPopulationFailures",
        TOTAL_GLYPH_POPULATION_FAILURES,
    ),
    ("totalGlyphPageRetirements", TOTAL_GLYPH_PAGE_RETIREMENTS),
    ("analyticGlyphResidentBytes", ANALYTIC_GLYPH_RESIDENT_BYTES),
    ("surfaceCacheRepaints", SURFACE_CACHE_REPAINTS),
    ("surfaceCacheReuses", SURFACE_CACHE_REUSES),
    ("surfaceCacheDirect", SURFACE_CACHE_DIRECT),
    ("surfaceCacheFallbacks", SURFACE_CACHE_FALLBACKS),
    ("surfaceCacheAnimated", SURFACE_CACHE_ANIMATED),
    ("surfaceCacheAllocations", SURFACE_CACHE_ALLOCATIONS),
    ("surfaceCacheEntries", SURFACE_CACHE_ENTRIES),
    ("surfaceCacheResidentBytes", SURFACE_CACHE_RESIDENT_BYTES),
    ("totalSurfaceCacheRepaints", TOTAL_SURFACE_CACHE_REPAINTS),
    ("totalSurfaceCacheReuses", TOTAL_SURFACE_CACHE_REUSES),
    ("totalSurfaceCacheDirect", TOTAL_SURFACE_CACHE_DIRECT),
    ("totalSurfaceCacheFallbacks", TOTAL_SURFACE_CACHE_FALLBACKS),
    (
        "totalSurfaceCacheAllocations",
        TOTAL_SURFACE_CACHE_ALLOCATIONS,
    ),
    ("guiLayoutReflows", GUI_LAYOUT_REFLOWS),
    ("guiTextMeasurements", GUI_TEXT_MEASUREMENTS),
    ("totalGuiLayoutReflows", TOTAL_GUI_LAYOUT_REFLOWS),
    ("totalGuiTextMeasurements", TOTAL_GUI_TEXT_MEASUREMENTS),
];

#[test]
fn layout_covers_every_word_once() {
    for (index, (_, word)) in NAMED_WORDS.iter().enumerate() {
        assert_eq!(*word, index, "words are dense and ordered");
    }
}

#[test]
fn worker_reader_mirrors_the_layout() {
    let reader = include_str!("../../../../packages/ipp-client/src/render-worker.ts");
    let start = reader
        .find("const RECORD = {")
        .expect("worker record layout");
    let end = start + reader[start..].find("} as const;").expect("layout end");
    let entries: Vec<(&str, usize)> = reader[start..end]
        .lines()
        .skip(1)
        .filter_map(|line| {
            let (name, offset) = line.trim().trim_end_matches(',').split_once(": ")?;
            Some((name, offset.parse().ok()?))
        })
        .collect();

    assert_eq!(entries, NAMED_WORDS);
    assert!(reader.contains(&format!("const RECORD_WORDS = {RECORD_WORDS};")));
}

#[test]
fn totals_accumulate_every_completed_render() {
    let mut record = RenderStatisticsRecord::new();
    let first = RenderStatistics {
        uploaded_bytes: 40,
        unshadowed_lights: 2,
        ..Default::default()
    };
    let second = RenderStatistics {
        uploaded_bytes: 2,
        ..Default::default()
    };
    record.accumulate(&first);
    record.accumulate(&second);

    let words = record.fill(&second);

    assert_eq!(words[UPLOADED_BYTES], 2);
    assert_eq!(words[TOTAL_UPLOADED_BYTES], 42);
    assert_eq!(words[UNSHADOWED_LIGHTS], 0);
    assert_eq!(words[FLAGS] & FLAG_GUI != 0, cfg!(feature = "gui"));
    assert_eq!(
        words[FLAGS] & FLAG_SURFACES != 0,
        cfg!(feature = "surfaces")
    );
    assert_eq!(words[FLAGS] & FLAG_SHADOWS != 0, cfg!(feature = "shadows"));
}

#[test]
fn totals_saturate() {
    let mut record = RenderStatisticsRecord::new();
    let large = RenderStatistics {
        uploaded_bytes: u32::MAX,
        ..Default::default()
    };
    record.accumulate(&large);
    record.accumulate(&large);

    assert_eq!(record.fill(&large)[TOTAL_UPLOADED_BYTES], u32::MAX);
}

#[cfg(feature = "gui")]
#[test]
fn layout_words_require_a_supported_host_sample() {
    let mut record = RenderStatisticsRecord::new();
    let statistics = RenderStatistics::default();
    assert_eq!(record.fill(&statistics)[FLAGS] & FLAG_LAYOUT, 0);
    record.layout = Some([1, 2, 7, u32::MAX]);

    let words = record.fill(&statistics);
    assert_ne!(words[FLAGS] & FLAG_LAYOUT, 0);

    assert_eq!(words[GUI_LAYOUT_REFLOWS], 1);
    assert_eq!(words[GUI_TEXT_MEASUREMENTS], 2);
    assert_eq!(words[TOTAL_GUI_LAYOUT_REFLOWS], 7);
    assert_eq!(words[TOTAL_GUI_TEXT_MEASUREMENTS], u32::MAX);

    record.layout = None;
    assert_eq!(record.fill(&statistics)[FLAGS] & FLAG_LAYOUT, 0);
}

#[cfg(feature = "gui")]
#[test]
fn host_layout_membership_and_words_share_the_same_sample() {
    let mut record = RenderStatisticsRecord::new();
    assert_eq!(record.layout_json(), "null");
    assert_eq!(record.layout_json_len(), 4);

    let mut host = ipp_core::HostRuntime::new();
    let identity = host
        .create_world(
            Default::default(),
            &[
                ipp_core::systems::canvas::CanvasSystem::ID,
                ipp_core::systems::gui::GuiSystem::ID,
                ipp_core::systems::gui::GuiLayoutSystem::ID,
            ],
        )
        .unwrap();
    let world = host.world_ref(identity).unwrap();
    let frame = host.frame(0.01).unwrap();
    record.record_frame(&mut host, &frame);

    assert_ne!(
        record.fill(&RenderStatistics::default())[FLAGS] & FLAG_LAYOUT,
        0
    );
    let json = record.layout_json();
    assert!(json.contains("\"scope\":\"host\""));
    assert!(json.contains("\"complete\":true"));
    assert!(json.contains(&format!(
        "\"world\":{{\"id\":\"{}\",\"incarnation\":\"{}\"}}",
        identity.0,
        world.incarnation()
    )));
    let length = json.len();
    assert_eq!(record.layout_json_len(), length);
}
