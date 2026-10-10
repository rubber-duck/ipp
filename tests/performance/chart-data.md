# Chart and data stress measurements

This opt-in benchmark exercises the release native GLES Host, its generated target contract, ordinary WebSocket dataset/World operations, Plot preparation and completed presentation frames. Scenarios own deterministic authoring and assertions; the native driver owns launch, transport, evidence and cleanup. They can be driven by a future worker arrangement without changing the workload.

The [measured before/after report](chart-data-results.md) records the exercised scope, CPU/allocation gains and remaining dense presentation limits.

```sh
python tools/ipp.py benchmark native --scene chart-data --egl-dir /lib64 --build-only
python tools/ipp.py benchmark native --scene chart-data --egl-dir /lib64 --reuse-build --preset smoke --repetitions 3 --samples 6 --output target/performance/chart-data-baseline
python tools/ipp.py benchmark native --scene chart-data --egl-dir /lib64 --reuse-build --preset full --repetitions 1 --samples 3 --warmup 1 --output target/performance/chart-data-full
```

Build before reserving the machine. Only one timing run may execute, with no concurrent builds or other stress workloads. Use the same release profile, workload, renderer and sampling parameters for comparisons. Software GLES requires `--allow-software` and must be identified as software evidence, never hardware performance. Benchmarks remain outside regression, including `--full`.

The smoke preset uses 1,000 source rows, one/four shared bindings, 1,000 lines/bars/points, a 10×10 surface, four/32 labels and a 1,000-row rolling stream. Full adds 10,000/100,000 data rows, 10,000 marks, a 32×32 surface and 128 labels. This is a representative ladder, not a Cartesian product. `--cases` selects comma-separated case names printed in the result workload records. Each case measures idle, a one-row edit (64-row append for streams), a typed computed parameter edit and a paused animation seek; 3D cases add camera rotation. Hosts retain their normal clocks. Paused controller seeks use the existing animation API and do not advance simulation manually.

Data cases evaluate the full-size source through independent bindings while a separate constant 1,000-row source paints one chart in the same World. Chart cases paint their declared mark count directly. This keeps source/binding scaling distinct from GPU geometry scaling. The initial unseparated 100,000-row/four-line case caused a Radeon context recovery and remains recorded as a discovered rendering limit; it is not rerun by the bounded presets.

Dense 3D presentation can take seconds per frame on this workload. Select `--modes idle,edit` to bound a dense comparison, then use the same case/mode/sample/warmup selection on both revisions. A deadline or manual interruption retains partial records and its termination reason; completed records are evidence for their own scope, not a passing whole sweep.

The full preset with default three repetitions/six samples may exceed the 12-minute deadline. It has not been established as a passing whole sweep; use explicit bounded sampling or case/mode selection. Surface edits change height while preserving the grid's x/z coordinates.

Limits are 100,000 source rows, four bindings, 128 labels, 64 samples, eight repetitions, 16 warmup cycles and a 12-minute driver deadline. Dataset ingestion is split into 1,024-row updates. Rolling retention remains exactly 1,000 rows after completed frames, with monotonically increasing stable row IDs. Binding observations independently assert source incarnation, row ID, row count, availability and shared computed values.

Setup and warmup are untimed. Each timing sample records command acknowledgement and the next completed frame fenced by the exact output and previous frame sequence. This includes transport and normal Host scheduling; it is not isolated CPU evaluation time or an FPS claim. Pixel transfer/PNG encoding, statistics queries, dataset/binding queries and RSS reads occur outside timing. Raw samples and p50/p95 are retained; no machine-specific timing threshold is used as a correctness gate.

Results identify the release executable, generated contract, fixture bundle, current source hash, CPU/OS/Node and actual graphics device. Each case retains a real native PNG with frame identity and meaningful chart-paint assertions. Failed draws and empty images fail the run. Host logs and partial results survive failure, and the driver closes connections, destroys Worlds/sources and terminates its Host.

Source `retainedBytes` describes accepted live rows; `allocatedBytes` describes source storage capacity. Neither is allocator traffic. Whole-process RSS includes the Host, renderer, caches and allocator residency, so a difference does not prove a leak. GPU upload/residency counters are read-only observations. Separate instrumented Rust diagnostics attribute allocation calls and requested bytes to bounded local data/chart operations; requested bytes are cumulative allocation demand, not retained memory. Normal release timings and instrumentation results must remain separate.

The local diagnostic products preserve normal release and instrumentation executables separately. Data diagnostics exercise actual Host frames and Data Service/binding work without transport/rendering. Chart diagnostics exercise preparation and camera-view label arrangement without GPU submission. They supplement the native frames; they do not replace them.

The data fixture belongs to `ipp-server/examples/data_profile.rs`, alongside the native Host profiler. Building a core-crate example would inherit core's test-only self-dependency and enable `checked-invariants`; the server-owned fixture avoids that composition. Verify the resolved core features for timing products, separately from the instrumentation product. Earlier data diagnostic records with that test feature are labeled explicitly in the results report.

```sh
python tools/ipp.py benchmark native --scene chart-data --diagnostic data --build-only
python tools/ipp.py benchmark native --scene chart-data --diagnostic data --reuse-build --preset full --samples 20 --output target/performance/data-timing
python tools/ipp.py benchmark native --scene chart-data --diagnostic data --instrumented --build-only
python tools/ipp.py benchmark native --scene chart-data --diagnostic data --instrumented --reuse-build --preset full --samples 20 --output target/performance/data-allocations
python tools/ipp.py benchmark native --scene chart-data --diagnostic chart --build-only
python tools/ipp.py benchmark native --scene chart-data --diagnostic chart --reuse-build --output target/performance/chart-timing
```

Use `--instrumented` with `--diagnostic chart` for its separate allocation product. Chart diagnostics run their maintained fixed 20-iteration fixture; `--samples` controls data diagnostic iterations. Diagnostic elapsed time, allocation calls and requested bytes are totals across iterations; divide by iterations for per-operation comparisons. Timing p50/p95 cover individual operations, while allocation-mode percentiles are absent. Build products and raw records retain their exact source identity.

Render diagnostics exercise the same real native workloads through a separate release instrumentation product, with allocation counters disabled, bounded CPU spans, asynchronous frame GPU queries and physical GL-call counts. Host frame/publication and renderer preparation/submission spans expose work outside World evaluation. They are inclusive nested spans, not additive totals. An untimed image readback after each sample window lets queued GPU queries resolve; its frames belong to the diagnostic capture, not the operation latency samples. GPU command intervals can include queue starvation caused by CPU work, so they do not prove active GPU execution throughout the interval. Pending queries at stop remain explicitly unavailable. These captures supplement ordinary release comparisons.

```sh
python tools/ipp.py benchmark native --scene chart-data --diagnostic render --egl-dir /lib64 --build-only
python tools/ipp.py benchmark native --scene chart-data --diagnostic render --egl-dir /lib64 --reuse-build --preset full --cases PlotPoints3d-10000,PlotLine2d-1000,PlotLine2d-10000 --modes idle,edit --samples 2 --repetitions 1 --warmup 1 --output target/performance/chart-render-profile
```

The opt-in renderer test `services::render::plot::label_layout::profile_tests::chart_path_scaling` inspects 1k/10k/100k analytic line packing entirely on the CPU. It reports original band lists, optional tile lists and their storage; it never submits the known failing 100k case to a graphics device. The shared [packing implementation](../../crates/ipp-render-gl/src/services/render/assets/paths.rs) owns the acceleration and conservative fallback rules.
