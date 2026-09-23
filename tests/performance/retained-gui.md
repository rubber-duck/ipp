# Retained Surface rendering measurements

Run `python tools/ipp.py benchmark browser --scene retained-gui --frames 60`. For this scene `--frames` counts streaming screen updates; Blender stress options such as `--preset`, `--group` or `--instrumented` are rejected. The opt-in workload runs the same maintained scenario as `python tools/ipp.py test retained-gui` with a longer streaming interval. It builds analytic Surface and GUI-enabled worker runtimes and drives both through generated clients, immutable font/drawing assets and actual WebGL. Results and completed-frame images go to `target/performance/retained-gui/`.

## Scenario

The terminal fixture presents the application's printable ASCII glyph set as positioned rows. Its generator accepts visible row/column counts, typing, cursor blink, scrolling, full replacement and an `unseen` mode that slides a window through Latin-1, box-drawing and icon glyphs never shown before. The scenario:

- counts cold misses and populations from counters accumulated over every rendered tick, then settles to a warm frame without uploads or rebuilds;
- derives the glyph vertex size from a full-screen replacement and bounds typing to one row and cursor blink to one quad;
- reuses local geometry for oblique and mirrored rear views of the same panel;
- shares font coverage across three panels, restores the graphics context, retires geometry for smaller screens and releases everything when cleared;
- grows the atlas past one page with unseen glyphs at a larger band, keeps it within the page budget while sliding through more glyphs than the budget retains, and proves eviction when the first window populates again;
- renders a GUI panel mixing a gradient shape with glow, atlas glyphs and a curve drawing, captures its mirrored rear view, and compares sparse and filled controls under one camera, viewport and DPR.

Before the atlas phase the scenario lowers the glyph atlas budget through the presentation channel (`setGlyphAtlasLimits`) and disables idle expiry. Pages retired while the windows slide are therefore pressure evictions: the scenario keeps every step within the configured budget and requires a positive accumulated `glyphPageRetirements` count. Pages still release when no World demands any glyph, so the cleared frame holds no atlas bytes.

Analytic builds lack the retained counters and report them unavailable, never as zero. The GUI counter `guiResidentBytes` is the combined GPU storage of retained box and glyph batches; `glyphResidentBytes` counts atlas pages separately. Vertex layouts are defined by `GlyphVertex` in [the glyph atlas](../../crates/ipp-render-gl/src/services/render/glyph_atlas.rs) and `GuiBoxVertex` in [the retained batch implementation](../../crates/ipp-render-gl/src/services/render/gui_batch.rs).

## Whole-Surface cache mode

`python tools/ipp.py benchmark browser --scene retained-gui --frames 60 --surface-cache` runs the same scenario with every terminal workload panel opted into whole-Surface caching and writes to `target/performance/retained-gui-surface-cache/` unless `--output` is given. The policy (`BENCHMARK_SURFACE_CACHE_POLICY` in [the scenario](../render/retained-gui-scenario.ts)) caches at every distance at the terminal view's screen density and uses the highest refresh cap, so each streamed update presents at its next frame while unchanged frames reuse the image. GUI control panels stay direct. Distance bands, refresh caps, interaction priority and lifecycle are asserted by the `surface-cache` suite rather than measured here.

`comparison.json` then adds `surfaceCache`: per build, the policy, the seven per-frame cache counters of the warm frame (`surfaceCacheRepaints`, `surfaceCacheReuses`, `surfaceCacheDirect`, `surfaceCacheFallbacks`, `surfaceCacheAllocations`, `surfaceCacheEntries`, `surfaceCacheResidentBytes`), the running totals after the last streamed update and the warm frame's cache records. Compare it with a run without the flag under the same machine, viewport and streaming count; a run whose renderer does not yet cache reports empty records and zero counters.

## Reports

`comparison.json` identifies its run: source revision with a hash of uncommitted changes, machine and Node identity, the pipeline run directory when started through `tools/ipp.py`, the streaming count, DPR, viewports and workload dimensions. Each build entry names its evidence directory, browser version and backend strings. Every label both builds capture has `comparisons/<label>-expected.png` (analytic), `-actual.png` (retained) and `-diff.png`, with the changed-pixel fraction, text-mask agreement and coverage ratio checked against the tolerances recorded beside them. Each build's evidence directory also holds its per-label captures and `workload.json`.

Timings measure the entire acknowledged application update through GPU readback. They include transport, scheduling and capture overhead and must not be reported as isolated rendering cost or FPS. Software GL proves correctness in its environment, not mobile performance.

## iPhone 14 Pro procedure

Physical-device measurements are pending a device; no iPhone result is implied by the automated browser suite. Use the maintained gallery on the device to compare the Aurora, Ember and Neon materials. Controls use retained shapes and Nerd Font icons; the waveform, reference grid and pulse remain Surface curve drawings:

1. Build the standalone gallery with `python tools/ipp.py build gallery-site`, then serve it with `python -m http.server 8000 --bind 0.0.0.0 --directory target/gallery-site`. On the same network, open `http://<development-machine-address>:8000/` in Safari and select the GUI world. Use a trusted HTTPS endpoint when exercising platform features that require a secure context. Stop the server with Ctrl-C after the run.
2. Record the source revision and uncommitted diff identity, runtime build identity, iOS/Safari version, display orientation, viewport/DPR, presentation-quality settings used (including whether **Isolate GUI panel only** is active), power mode and thermal state. Keep these fixed between samples. Disable Low Power Mode and allow the device to cool between runs.
3. Wait 10 seconds for assets and atlas warmup. Measure 30 seconds with Safari's remote Web Inspector Timelines recording while the panel faces the camera. Repeat with an oblique panel at the same projected size. Keep camera positions, animation and control state identical between skin samples.
4. Repeat while dragging the slider, scrolling, typing in the text field and blinking the caret. Record frame cadence distributions and long frames from the timeline, plus the exact recording interval and instrumentation. Save screenshots of idle, hovered/pressed where available, focused and disabled controls, and resize/orientation changes. Do not infer keyboard/IME acceptance from rendering measurements.
5. Repeat each sample three times and retain raw recordings with screenshots and device details. Report thermal drift and unavailable measurements explicitly. The automated terminal capture timings and interactive Safari cadence measure different intervals and should remain separate.

This procedure does not measure whole-Surface texture caching or cache transitions; see [optional Surface texture caching](../../docs/architecture/rendering.md#optional-surface-texture-caching) and the cache mode above for the automated comparison.
