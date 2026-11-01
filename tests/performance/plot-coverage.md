# 2D Plot rendering coverage and scaling

The [Apple M4 results](plot-coverage-results.md) record the generalized renderer’s initial comparison.

This benchmark measures ordinary worker/WASM/WebGL presentation through real generated clients and Data Service bindings. It exercises compact strokes, multiple series, markers, bars, pie sectors, areas, bands and authored paint with deterministic source rows. The maintained `plots` suite separately checks rendered coverage, transparency, source-row picking, live edits, curved Surfaces and context recovery on native GLES and WebGL.

```sh
python tools/ipp.py benchmark browser --scene plot-coverage --build-only --hardware metal
python tools/ipp.py benchmark browser --scene plot-coverage --reuse-build --preset full --samples 2 --repetitions 3 --warmup 2 --hardware metal --output target/performance/plot-coverage-current
```

Use the hardware backend appropriate to the machine. Timing requires hardware rendering and an ordinary build without instrumentation. Run serially on an idle machine, without competing graphics applications or builds. The launcher requests uncapped presentation; results count completed presentation sequences per elapsed second, not physical display refreshes or isolated GPU execution time. Each sample includes the ordinary Host and browser scheduling. The fixed viewport is 1600×1000 at DPR 1.

The smoke preset uses 600 samples; full also measures 2,400. `--cases` selects comma-separated `line`, `multiline`, `markers`, `bars`, `pie`, `area`, `band`, `filled-markers`, `multiband` and `painted-band`. Multiline draws four series; multiband draws four fully overlapping, independently colored bands to expose transparency cost. Source values include rapid changes and baseline crossings; sample reduction is disabled. Filled cases retain their upper stroke. The painted band uses a portable Canvas paint with a contour highlight. These workloads vary primitive count and complexity; they do not represent equal painted area or equal fragment work.

Data and paint readiness, warmup, diagnostics and screenshots are outside measurement windows. Reports preserve all samples, actual device/compositor information, ordinary runtime and generated-contract identities, fixture hashes, source changes, machine state, frame work counters and nonempty image assertions. Failed draws, unavailable hardware, missing readiness and empty captures fail the run. The runner owns its browser, server, worker, Worlds and sources, and preserves partial reports on failure.

For comparisons, preserve the earlier ordinary browser product before rebuilding. Set `IPP_PLOT_BENCHMARK_RUNTIME` to its directory and reuse the same built benchmark fixture. The comparison loads that product's own generated client and row layout; features absent from that contract are explicitly unsupported. Areas and bands have no old implementation to time. Compare supported cases with matching viewport, data, timing parameters and hardware, and retain the identity of both builds. A preserved build's recorded dirty source identity must not be relabeled as a clean commit.

Statistics include unchanged-frame uploads and retained GUI rebuild/allocation counters. They are not a global allocator measurement. The `render-publications:frame-allocations` test separately checks zero allocations across unchanged Host frames containing smooth filled lines, multiple series and markers. Loading and changed-data preparation may allocate. More visible geometry, overlapping transparent series and arbitrary authored paint require additional work; desktop throughput is not evidence of phone or headset performance.
