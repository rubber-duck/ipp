# Chart and data measurements — 3 October 2026

The [maintained benchmark](chart-data.md) establishes bounded native stress evidence and identifies two useful CPU improvements: unchanged count/non-clock-driven range streams no longer scan retained rows on every clock update, and exact identity columns avoid expression evaluation. Host-time ranges still expire on clock updates. Chart preparation reuses label-placement scratch and skips unused normal calculations for unshaded edges. The [profiling follow-up](#dense-chart-profiling-follow-up) below identifies and removes repeated publication comparisons and narrows dense analytic lookup work; the earlier tables retain their original measurements.

## Measurement scope

Hardware was an AMD Ryzen 9 5900X, 24 logical CPUs, with an AMD Radeon RX 9070 XT (`radeonsi`, `gfx1201`, ACO), OpenGL ES 3.2 Mesa 26.1.8, Linux 7.2.6-arch2-1 and Node 22.22.2. Native frames were 640×400. Builds and timing windows ran separately, with one timed workload at a time. Timings have no machine-specific pass threshold.

Before runtime: `67d53e4b`. After runtime: data `fdab8c21` and chart `239380b6`; diagnostic-only follow-ups are `d8dd12b3` and the unchanged data-fixture move `1c78cf30`. Native after build revision `11b5f97b` has the same runtime source files as integrated `74c328c7`. Its release executable SHA256 is `7afb1dbe192e5fa3a16b866582d214fd1e853ea6441b9b038bf7ef4d33d15fb6`; the before executable is `5620332a09b7ce6cdb5616499abccc1c58d2c08817ad7f7bce5863aebb2a58ae`. Both use contract SHA256 `f74b1b15df8cfcfc14da2d91a4e84b7d33d2e41e092b645554e59474529cfac6`.

Native timing uses ordinary release builds without instrumentation. Local data and chart timing products also verify resolved core features `[]`; separate allocation products enable only `instrumentation`. Earlier core-owned data-example records inherited the test-only `checked-invariants` feature and are supplemental release-with-checked-invariants evidence, excluded from the production data table below. Moving the identical fixture to `ipp-server/examples/data_profile.rs` corrects that build composition.

## Completed native frames

The interval starts before an edit and ends at its next fenced completed frame, including acknowledgement, transport and the Host's ordinary scheduling. It is not isolated evaluation time, GPU time or display FPS. Each row has one matched repetition. Main and dense cases have six samples and two warmups; grid bars, surface, labels and rolling stream have four samples and one warmup. Quantiles use nearest rank; p95 is the maximum in these short windows.

| Workload / operation           | Before p50 / p95, ms | After p50 / p95, ms |
| ------------------------------ | -------------------: | ------------------: |
| 1k rows, four bindings, edit   |      66.860 / 67.295 |     66.846 / 66.904 |
| 10k rows, four bindings, edit  |      66.833 / 67.907 |     66.852 / 66.986 |
| 100k rows, four bindings, edit |    108.607 / 127.452 |   104.380 / 116.991 |
| 10k 2D bars, edit              |      66.897 / 69.520 |     66.849 / 70.294 |
| 1k grid bars, edit             |      76.454 / 83.560 |     75.257 / 83.539 |
| 32×32 surface, edit            |      66.765 / 69.219 |     66.671 / 68.905 |
| 128 labels, edit               |    106.469 / 115.033 |   102.591 / 118.427 |
| 128 labels, camera             |      33.392 / 46.262 |     33.394 / 43.840 |
| 1k rolling rows, 64-row append |      66.878 / 67.693 |     66.864 / 67.663 |
| 10k outlined points, idle      |  1555.134 / 1626.293 | 1550.769 / 1605.750 |
| 10k outlined points, edit      |  5291.455 / 5349.715 | 5144.187 / 5234.755 |

All 37 matched operation records are retained, including parameter edits and paused animation seeks. Most idle frames remain near 16.7 ms and parameter/camera observations near 33.4 ms; scheduling hides smaller local CPU gains. These short native windows establish behavior and scaling limits, not a general speedup claim. The dense point case still takes roughly 1.55 seconds idle and 5.14 seconds after an edit; the measurements do not separate GPU execution from CPU/driver presentation costs.

Source incarnation, stable row IDs, availability, computed values and retained row counts passed independent assertions. The rolling source retained exactly 1,000 rows and 256,000 live bytes, with 296,000 allocated source bytes; final Host RSS was 157,401,088 bytes. RSS includes caches, renderer and allocator residency. It is not requested allocation traffic or proof of leak freedom.

Eight same-final-state native PNGs are byte-identical before/after: all three data scales, 2D bars, grid bars, surface, 128 labels and rolling stream. Actual after surface, dense-label and dense-point captures were inspected at original resolution. The 128-label image deliberately retains crowded stress content. Dense points after ends after idle/edit, whereas before also changed parameters, animation and camera; those final PNG hashes are not compared. Maintained native 3D/view scenarios separately passed real picking and image assertions on the optimized runtime.

## Local CPU and allocation attribution

These operations exclude transport and GPU presentation. The data table uses median p50/p95 across three alternating matched production release runs, pinned to CPU 3: 200 operations for 100k-row idle, 50 for 1m-row idle and 20 for binding edits.

| Local data operation           | Before p50 / p95, ms | After p50 / p95, ms |
| ------------------------------ | -------------------: | ------------------: |
| Count-window idle, 100k rows   |  0.479056 / 0.485927 | 0.000770 / 0.000800 |
| Count-window idle, 1m rows     |  4.655304 / 4.716016 | 0.000760 / 0.000810 |
| 100k rows, four bindings, edit |      27.072 / 27.701 |     21.288 / 22.715 |

The unchanged-stream slope becomes approximately flat instead of increasing tenfold with retained rows. The dirty four-binding case improves p50 by 21.4%; it still recomputes the full changed binding outputs.

The corrected production allocation pair uses core features `[instrumentation]` without `checked-invariants`. Four-binding edits decrease total allocation calls from 21 to 17 and requested bytes from 6,808 to 6,776 per operation. The DataBinding evaluation stage accounts for the reduction: eight calls/96 requested bytes become four calls/64 bytes per frame. Count-window idle remains 11 calls/6,576 requested bytes per operation. Retained source rows and bytes are unchanged. Stage counters describe the separate instrumented window, not production timing.

Chart diagnostics report median quantiles across three release runs of 20 operations. Historical chart p50 uses upper-median rank 11/20 and p95 rank 19/20; the maintained fixture now uses nearest-rank p50 rank 10/20. This reporting correction does not change the runtime or invalidate the preserved comparison.

| Local chart operation | Before p50 / p95, ms | After p50 / p95, ms | Allocation calls before → after | Requested bytes before → after |
| --- | --: | --: | --: | --: |
| 128 generic callouts, idle arrangement | 1.777 / 1.912 | 1.035 / 1.170 | 21,814 → 132 | 1,704,264 → 83,888 |
| 512 generic callouts, idle arrangement | 13.419 / 13.925 | 9.496 / 10.008 | 112,571 → 334 | 23,260,544 → 335,184 |
| 10k grid bars, preparation | 70.689 / 71.083 | 60.630 / 61.336 | 3,548 → 3,548 | 247,647,416 → 247,647,416 |
| 10k points, preparation | 70.388 / 71.532 | 60.632 / 61.694 | 3,548 → 3,548 | 247,647,416 → 247,647,416 |
| 32×32 grid surface, preparation | 1.840 / 1.867 | 1.257 / 1.279 | 894 → 894 | 7,361,813 → 7,361,813 |

Allocation figures are per operation from separate five-operation instrumented windows. Requested bytes are cumulative allocation demand, not retained heap, source capacity or RSS. Geometry retains its complete mesh; the preparation improvement does not reduce resident geometry or make dense rendering fast. Raw chart records include unchanged line/smooth-line/2D-bar operations, radial labels and camera cases rather than selecting only winning cases. Instrumented timing is not used for speed claims.

## Limits and retained evidence

The initial 100k-row/four-line rendering experiment caused an actual Radeon context loss and hard recovery, ending the Host with `SIGABRT`. Its stderr reports `The CS has cancelled because the context is lost`. That case was not rerun. The bounded benchmark separates 100k-row data evaluation from a constant 1k-row visual source; it does not establish support for 100k rendered line marks. Beads `ipp-u0na.27` tracks the dense analytic-path limit, and `ipp-u0na.28` tracks dense outlined-point presentation.

The long initial bounded sweep was intentionally interrupted after one complete repetition of five cases. Its 21 completed mode records are valid for that scope, not a passing whole sweep. Remaining before cases ran separately. An obsolete surface component name and a fixture edit that duplicated grid coordinates produced retained failures; corrected height-only surface before/after runs both passed. The full default sweep remains unvalidated and may reach its deadline.

Artifacts are under `/home/dev/ipp/.worktrees/ipp-u0na/worker-logs/`: `chart-stress-native-paired-comparison.json` contains all 37 matched records and eight PNG SHA256 comparisons; `chart-stress-native-{bounded-baseline,baseline-completion,baseline-final-small}/results.json` are the accepted before scopes; `chart-stress-native-after-{main,small,dense}/results.json` are the three passing after runs. Each run retains source/build/device identity, raw samples, memory observations, captures and Host logs. The before fixture SHA256 is `d2325064b3b9d9e7178bfe04d427ee250bc5cbb05c70b949c6996b85046a7ee0` for main/grid cases; corrected small-before and all-after use `0678714a3c7634caf176f8e31f08166437b178588e2797c3cace6e5f2e897537`. Untimed verification/capture guards and mode selection changed the bundle; surface corrections changed only its invalid workload. The compared main/grid workload remains unchanged.

Local raw evidence is `data-profile-production-timing-metrics.json`, `data-profile-production-allocation-metrics.json`, `chart-profile-paired-comparison.json`, the corresponding timing/allocation logs and immutable binaries, `chart-profile-paired-identity.json` and `chart-profile-release-feature-fingerprints.json`. The original hard-reset scene/build is preserved in `chart-stress-unbounded-baseline-build.tar.gz`, SHA256 `8c84829e10fbddde2eee391fb9a2915225c71234b95e27c2cc50c46261885b7e`, with `chart-stress-native-baseline/host-stderr.log`. No failing or interrupted whole sweep is advertised as passing.

All after Hosts exited and their PIDs were absent. Required affected regression passed across integration runs `run-6w3hmsld` and `run-4xkkv2rf`, including native/worker datasets, native Plot 3D/views, Plot core/render tests and all 108 pipeline tests. Benchmarks remain opt-in and excluded from full regression. See the [guide](chart-data.md) for bounded reproduction commands and separate instrumentation products.

The three after commands used the previously built release product:

```sh
python tools/ipp.py benchmark native --scene chart-data --egl-dir /lib64 --reuse-build --preset full --repetitions 1 --samples 6 --warmup 2 --cases data-1000-shared-4,data-10000-shared-4,data-100000-shared-4,PlotBars2d-10000 --output target/performance/chart-after-main
python tools/ipp.py benchmark native --scene chart-data --egl-dir /lib64 --reuse-build --preset full --repetitions 1 --samples 4 --warmup 1 --cases PlotGridBars3d-1000,surface-32x32,labels-128,rolling-1000 --output target/performance/chart-after-small
python tools/ipp.py benchmark native --scene chart-data --egl-dir /lib64 --reuse-build --preset full --repetitions 1 --samples 6 --warmup 2 --cases PlotPoints3d-10000 --modes idle,edit --output target/performance/chart-after-dense
```

## Dense chart profiling follow-up

After advanced profiling landed on main `21b86c6c`, the same hardware and viewport were used for bounded before/after measurements. Ordinary release runs used one repetition, three samples and one warmup per idle/edit mode; separate release instrumentation captures used two samples, allocation counters disabled, named CPU spans and frame GPU queries. Before runs preceded the implementation; they were not an alternating A/B campaign. These short windows describe this fixture and machine, not general FPS or mobile performance.

### Outlined 3D points

The expensive path was CPU publication equality, rather than the triangle count alone. Each mesh chunk retained the complete chart geometry; derived equality rescanned that geometry for every chunk on every publication. Four baseline idle frames spent 3,047.67 ms in `PlotSystem.publish` out of 3,052.43 ms in `HostRuntime.frame`, versus 1.50 ms of renderer CPU submission. Publication now compares retained allocations directly and compares distinct geometry once per chart. Equal rebuilt geometry reuses its previous immutable allocation after one evaluation-time comparison, preserving value equality and ordinary invalidation.

| Workload / operation      | Before p50 / p95, ms | After p50 / p95, ms |
| ------------------------- | -------------------: | ------------------: |
| 1k outlined points, idle  |      16.661 / 26.768 |     16.642 / 17.285 |
| 1k outlined points, edit  |      83.467 / 92.525 |     83.609 / 84.631 |
| 10k outlined points, idle |  1555.130 / 1557.176 |     16.704 / 18.813 |
| 10k outlined points, edit |  5938.539 / 6430.601 |   173.310 / 177.733 |

The final idle capture spent 0.109 ms in publication across six frames. Geometry remains 156 triangles per outlined cube, with 1,560,410 total submitted triangles at 10k points including other scene content. Idle uploads remain zero, and all four initial/final point PNGs match the baseline byte for byte. Available median GPU frame intervals remain approximately 0.4 ms; long baseline query intervals can include CPU queue starvation and are not evidence of equivalent active GPU work. The final edit capture attributed 120.98 ms to 144 mesh-upload calls across two edits; idle submission used 1.67 ms across six frames. These upload scopes include CPU packing, driver submission and any synchronous driver wait, without isolating those costs. Real edits still rebuild and upload mesh streams; this change does not reduce geometry or establish a cheap arbitrary-density edit path.

### Dense analytic lines

The original 16 axis bands produced candidate lists that grew with source size. A conservative 64×64 lookup now filters each curve by its perpendicular hull and its entire closed contour's ray-axis hull. Entire distant contours have cancelling winding and no antialiasing contribution. Lists retain curve order, and the shader still draws one analytic union with the same fill rule, overlap alpha and antialiasing. Large pixel footprints use the original bands. Optional storage is bounded relative to band storage, and an atlas exceeding the device texture extent is repacked with original bands before upload, including subsequent paths' descriptor offsets.

| Workload / operation | Before median GPU interval, ms | After median GPU interval, ms |
| --- | --: | --: |
| 1k line samples, idle | 6.915 | 0.545 |
| 1k line samples, edit | 7.050 | 0.587 |
| 10k line samples, idle | 42.242 | 3.624 |
| 10k line samples, edit | 35.471 | 3.827 |

These are available hardware query samples from separate diagnostic captures, not ordinary application latency. Pending and unavailable records remain explicit; the capture driver drains query results outside the timing window. Final captures dropped no CPU spans or GPU records and retain World identity for publication and shared Host identity for renderer work. All eight line PNGs match the baseline byte for byte. Maintained native GLES and worker WebGL scenarios additionally assert dense alpha overlap, restored paint, edited paint and final source-row picking, including minified placement.

At 10k samples, maximum horizontal/vertical candidates fall from 32,350/12,772 to 1,676/1,252. Packed atlas data rises from 5,623,368 to 18,649,720 bytes, excluding device row padding and temporary CPU allocations. CPU-only 100k packing retains all 299,997 contours and 2,299,977 curve texels; its tiled atlas is 185,587,428 bytes. No 100k GPU submission was attempted. The historical Radeon hard-recovery evidence is preserved unchanged; bounded results support the excessive-fragment-work diagnosis but do not prove a unique driver-reset cause or a safe 100k GPU workload.

### Reproduction and identities

Use the commands in the [benchmark guide](chart-data.md) with `--cases PlotPoints3d-1000,PlotPoints3d-10000 --modes idle,edit --samples 3 --repetitions 1 --warmup 1` for ordinary release timing. Use `--diagnostic render --cases PlotPoints3d-10000,PlotLine2d-1000,PlotLine2d-10000 --modes idle,edit --samples 2 --repetitions 1 --warmup 1` for the separate capture product. CPU-only path scaling remains the ignored renderer test documented in that guide.

The ordinary before executable SHA256 is `07599a819e2976442b6f8c2ca0de87035190ced54cf8dfbab35dc7a9dae7ded7`; final is `4ec4f0a98d5c50a52a3a500e0dd319f1e0be808ba2139f7bfcf0ffd5a702f3ea`. Both use contract SHA256 `200e8aa4c191d35d77d03a3f69d90070d3b108d67ad8c6672fdc2dbedbd7e89d`. Final ordinary source identity is `627f390ce829d09440df6ec09b5f7139bc8b0a998cd24354c5249dfe72af4d5f` on base `21b86c6c`; report-only edits follow the measured build. Raw reports retain the instrumentation build identity separately.

Local raw evidence, before/after captures, PNGs, historical failure hashes and `comparison-device-final.json` are preserved under `/home/dev/ipp/.worktrees/ipp-u0na.27/worker-logs/dense-charts/`. Final measurement pipeline reports are `run-ek02xc9r` and `run-iawcrysk` in the implementation worktree's `target/pipeline/runs/`. These artifacts are local measurement evidence; no remote publication is implied.
