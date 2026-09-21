# Historical pointer experiment

The newer [Blender stress benchmark](stress.md) and [measured results](stress-results.md) cover the 10,000-cube scene on the current integration base. Native CPU/GLES timings and mixed-property comparisons are in [native results](native-results.md).

For the newer full-frame allocation sweep, queue reuse, callback-preserving mode 3 and current validation, see [Per-frame allocation sweep](allocations.md). The results below are the earlier pointer experiment and remain historical evidence.

Measured 2026-09-14 on an AMD Ryzen 9 5900X, Linux, Rust 1.98.1 and Node 22.22.2. These measurements describe the original experimental build. Later adopted optimizations and current commands are described in the [benchmark guide](stress.md).

## Source and interpretation

The worktree `/home/dev/ipp-worktrees/performance-experiments`, branch `codex/performance-experiments`, starts at `355a2da` plus a verified snapshot of the primary checkout's uncommitted changes taken at approximately 10:04 UTC. The original checkout remains independent. `target/performance/baseline-manifest.json` and `inherited.patch` identify that snapshot. Later changes in the primary checkout are not included. Beads `ipp-5ke` owns the experiment and handoff.

The native example uses actual core controllers, uploaded/decoded clips, stable component storage and the normal Host/World update schedule. Its isolated codec measurement uses the production decoder and verifies the resulting batch. These are core/codec measurements, not transport latency measurements. The robot harness uses the maintained gallery environment, generated client, worker transport, actual WASM, disk assets, WebGL, asynchronous readiness and completed frame capture.

All timings use optimized artifacts: native `release` (opt-level 3), browser the existing `release-small` (opt-level s, thin LTO) distribution. Counters are disabled during timing runs and collected separately. The experiment allocator's enabled check and the disabled phase guards remain compiled into all compared variants. These are instrumented-build comparisons, not claims about an uninstrumented shipping binary. Native timing is pinned to CPU 22. This is a shared workstation with frequency scaling and other activity; repeated opposite-order comparisons limit drift but do not provide laboratory isolation.

The three variants share the same sampling, controller traversal and staging code:

| Mode | Change |
| --- | --- |
| 0 | Existing animation restoration and evaluated component commit |
| 1 | Validated typed assignment for Scalar, Transform, UnlitMaterial, PbrMaterial and Light; other components retain existing commits |
| 2 | Same as mode 1, with the final typed assignment expressed through a raw pointer |

The component experiment bypasses general evaluated commit callbacks for those five resource-free, Copy component types. It retains temporary component maps and validates the complete component before assignment. It is **not production-ready**: arbitrary extension System commit observers have not been preserved or verified, and it is not a general replacement for discrete/resource/property lifetime transitions. Dynamic properties and joint sampling still use their existing paths. A production optimization must preserve dependency invalidation and observer semantics while avoiding unnecessary structural work.

The raw pointer is derived from a current exclusive typed borrow and consumed immediately. It never survives an allocation, lifecycle callback, component replacement or World replacement. A separate microbenchmark compares indexed boxed slots, cached disjoint safe references and cached raw pointers. That narrow benchmark has fixed, unique targets; it does not prove persistent pointer safety in the runtime.

## Native update results

Values below are medians of two opposite-order blocks in one process. Each block averages 100–5000 warmed updates, depending on size. Controllers are paused at 0.5 seconds but retain active contributions, so every update restores and samples the actual animation pipeline. Idle Worlds contain the same entities and an uploaded unused clip.

| Workload | Existing update | Safe direct commit | Raw pointer commit | Existing → direct allocation calls/update |
| --- | --: | --: | --: | --: |
| 1 scalar, 1 controller | 4.37 µs | 2.66 µs | 2.65 µs | 55 → 43 |
| 64 scalars, 1 controller | 1.238 ms | 0.0315 ms | 0.0315 ms | 17,398 → 502 |
| 512 scalars, 1 controller | 84.40 ms | 0.304 ms | 0.303 ms | 1,056,458 → 3,786 |
| 64 scalars, 64 controllers | 1.357 ms | 0.0601 ms | 0.0618 ms | 17,525 → 629 |
| 512 scalars, 512 controllers | 95.09 ms | 0.444 ms | 0.447 ms | 1,057,408 → 4,736 |
| 512 idle scalars | 8.11 µs | 8.08 µs | 8.09 µs | 6 → 6 |

With 512 scalar drivers in one controller, requested allocation traffic falls from approximately 194.3 MB to 0.816 MB per update. These are allocation/reallocation requests, **not retained memory or a leak**. Fractional allocation counts for many-controller workloads reflect B-tree node churn across frames.

Increasing drivers from 64 to 512 multiplies baseline time about 68 times and allocations about 61 times. That is consistent with the quadratic validation path visible in the source, not a costly final pointer dereference. Safe versus unsafe direct writes show no material difference.

The isolated fixed-target sampler/write benchmark measured 9.88 ns per indexed write, 9.66 ns with cached safe references and 9.58 ns with cached raw pointers (medians of six blocks; 512 targets, 10,000 frames per block). This does not establish a useful unsafe-specific advantage. It omits World identities, lifecycle, assets and restoration, so its nanosecond figures must not be presented as complete driver costs.

A two-key scalar sample measured 11.7 ns for linear interpolation and 631 ns for Bézier interpolation. The current Bézier time solver always performs 52 bisection iterations. This is a separate math cost; the robot results below do not establish that Bézier solving dominates its animation.

## Robot results: identical held pose

The real demo contains 69 entities, 8 controllers and 41 declared drivers, including two joint-target declarations. Some choreography controllers are stopped. It renders 43 draws and 95,101 triangles at a 718 × 539 framebuffer in Chromium 153.0.8010.12 using SwiftShader/WebGL 2. Uploaded assets total approximately 16.3 MB at the captured frame; steady captures report no new uploads.

The application Pause control freezes the pose while normal restoration, sampling and rendering continue. Six mode blocks run in order 0, 1, 2, 2, 1, 0, with 15 warmup frames, at least 100 timed frames and 20 separately profiled frames per block. There are no inspection requests or frame captures inside the measured blocks. Every variant has exactly equal inspected entity/controller state and zero differing pixels against the baseline completed frame.

| Metric | Existing | Safe direct commit | Raw pointer commit |
| --- | --: | --: | --: |
| Timed frames | 202 | 204 | 202 |
| Median Host tick, including rendering/submission waits | 22.5 ms | 22.0 ms | 21.6 ms |
| Mean Host tick | 23.40 ms | 22.58 ms | 22.85 ms |
| p95 Host tick | 31.4 ms | 29.0 ms | 32.1 ms |
| Approximate sum of instrumented core phases | 2.35 ms | 1.89 ms | 1.71 ms |
| Allocation calls/frame | 28,046 | 17,456 | 17,456 |
| Requested allocation bytes/frame | 5,413,229 | 3,361,293 | 3,361,293 |

The deterministic allocation improvement is 37.8%. The median Host tick improvement is only about 2–4%, and the mean/p95 ordering does not establish an unsafe advantage. Coarse browser clock precision and short instrumented windows mean the core phase times are approximate. Do not interpret them as proof that pointer syntax is faster than the safe write.

Baseline animation restoration and evaluation together occupy about 2.0 ms of the approximately 2.35 ms measured core phases and account for 26,847 of 28,046 allocation calls per frame. The remaining time inside the Host tick includes actual rendering and graphics submission waits. This is software rendering; these measurements cannot predict hardware GPU FPS, presentation latency or display cadence. GPU completion is asserted for correctness captures, not forced inside every timing sample.

The held-pose evidence is in `target/integration-artifacts/gallery-combined/ipp-browser-robot-performance-i66TRb/`: `profile.json`, `build-identity.json`, frame images, events and browser logs. No explicit simulation-step command was added to the client protocol.

## Robot results: moving playback

A second real-browser run resets and plays the six active walk/ambient controllers before each block; the turn/scratch controllers remain stopped. It uses the same reverse-order comparison with 15 warmup, at least 50 timed and 20 separately profiled frames. All six blocks verify advancing controllers, changed entity state and meaningful pixel changes. Timed walk positions are comparable but not frame-identical because the production Host clock remains autonomous.

| Metric | Existing | Safe direct commit | Raw pointer commit |
| --- | --: | --: | --: |
| Timed frames | 102 | 104 | 104 |
| Median Host tick | 22.3 ms | 20.9 ms | 21.6 ms |
| Mean Host tick | 23.56 ms | 22.27 ms | 22.32 ms |
| p95 Host tick | 33.2 ms | 32.3 ms | 27.4 ms |
| Approximate sum of instrumented core phases | 2.40 ms | 1.75 ms | 1.88 ms |
| Allocation calls/frame, approximate | 28,084 | 17,512 | 17,508 |

The moving-scene median tick improves about 6.3% with the safe variant and 3.1% with the raw-pointer variant; mean tick improves about 5% for either. The ordering reverses relative to some held-pose metrics, reinforcing that no unsafe-specific gain was established. Allocation counts remain about 38% lower. Animation still accounts for most measured core work; actual skinning is approximately 0.05–0.10 ms in these instrumented windows, far below animation restoration and evaluation. These are Host tick measurements, not achieved display FPS.

Moving evidence: `target/integration-artifacts/gallery-combined/ipp-browser-robot-performance-dxpfUz/`. The later profiler build additionally records actual System names for each schedule position, including schedules reconstructed from a saved World; its separate build identity is retained. No animation algorithm or experimental numeric write changed between the held and moving builds.

## Hotspots and recommended changes

1. **Stop validating all animation bindings for every numeric component commit.** [Animation sampling/restoration](../../crates/ipp-core/src/world/systems/animation/update.rs) stages a component then invokes [apply_evaluated_value and commit_components](../../crates/ipp-core/src/world/access.rs). Each commit invokes every selected System's validation and before/after callbacks. [validate_animation_changes](../../crates/ipp-core/src/world/systems/animation/update.rs) scans all bound drivers again. Restoring and sampling N independently targeted components can therefore trigger approximately 2N full driver scans per update. Keep synchronous lifetime invalidation, but index affected bindings and use a bounded numeric evaluation path whose dirty-output behavior is explicit. This has the strongest measured benefit.

2. **Replace inspection-style property reads with generated single-field access.** [AnimationValue::read](../../crates/ipp-core/src/world/systems/animation/clip.rs) calls `component.fields()`, allocates a complete field list, then allocates another vector for the selected values. Dynamic-property inspection can enumerate and clone an entire property set. `AnimationValue::write` also allocates a vector for a scalar or four quaternion elements. A generated exact-field getter and direct scalar/fixed-array writes can remove these allocations with safe Rust. Keep enumeration for actual inspection. The quadratic validation loop amplifies these small allocations into the million-allocation synthetic case.

3. **Reuse animation scratch storage and stable activation metadata.** `changed`, its HashSet, readiness/failure sets, controller ID vectors and per-controller component B-trees are rebuilt. Controllers are removed/reinserted in B-trees during restore/sample; many-controller measurements expose the extra cost. Source demand is rebuilt and source strings are cloned per driver. Reuse capacity, keep deterministic flat active-driver/controller traversal, cache controller duration and deduplicated clip demand, and refresh only at changes. Resolve typed tracks once where source-residency rules permit. Preserve paused reapplication, exact property coverage and baseline inheritance.

4. **Reduce joint and skinning scratch allocations.** [Joint read/write](../../crates/ipp-core/src/world/systems/animation/driver.rs) and [pose interpolation](../../crates/ipp-core/src/world/systems/animation/pose.rs) materialize temporary pose vectors; full-weight nonadditive drivers still read current values. Additive drivers resample an immutable reference pose each frame. [Skinning](../../crates/ipp-core/src/world/systems/skinning/update.rs) allocates a palette vector then copies it into the already stable palette buffer. Reuse scratch or write into validated existing destinations, cache immutable reference samples, and retain all-or-failed-controller behavior. These are source-confirmed opportunities; this experiment does not separately quantify each one.

5. **Optimize mutation and delivery before inventing a new wire format.** Production decoding of 512 scalar SetField operations (10,781 wire bytes) takes 16.7 µs and one 90,112-byte operation-vector allocation. Preparing/cloning a changing batch, enqueueing it and running the real core update takes 1.233 ms and 4,449 allocations. At 64 operations those figures are 2.10 µs versus 98.5 µs. The latter measurement includes command preparation and the complete core step; it excludes transport, reply encoding and GPU rendering. `Command` uses 176 bytes per operation in this native build, much larger than its scalar wire form. [Operation dispatch](../../crates/ipp-core/src/world/mutation.rs) traverses System callbacks per operation; [Host tick](../../crates/ipp-host-session/src/lib.rs) materializes session/World collections and clones complete reports per attached session. Borrow reports, reuse output/encoding capacity and measure batching/fanout through real transports before adding shared memory or changing framing. Related primary-checkout work is already active in Beads; this experiment does not replace it.

6. **Measure curve and rendering-specific improvements after removing structural overhead.** A safeguarded Newton solve plus bounded bisection fallback could reduce the 52-step Bézier solver; evaluate numerical error against the existing exact-seek and curve tests before changing it. Cache repeated quaternion/reference calculations where valid. The robot's much larger total tick than measured core phase time also merits hardware GPU profiling, particularly shadow rendering and submission, before attributing frame rate to animation.

## Persistent pointers: requirements before a production rewrite

A retained target pointer must be bound to the exact World, entity generation, component incarnation and property coverage. Every destruction, replacement, asset-source change, pose-buffer replacement and World replacement must invalidate it before reuse. Dynamic properties specifically retain validated identities and offsets rather than pointers across buffer relocation. A retained source-track pointer also needs its clip payload lifetime preserved across unload/reload. Mutation callbacks must never overlap active Rust borrows into those targets. Overlapping writers still follow deterministic controller/description order.

Stable addresses alone do not prove aliasing safety. The isolated cached-pointer microbenchmark has deliberately simpler ownership. The robot experiment uses only immediate pointers, so it establishes neither the correctness nor the performance of a complete persistent-pointer driver rewrite. The measured evidence supports removing repeated validation, field enumeration and allocation first; safe access can achieve the demonstrated direct-commit speedup.

## Reproduction

From the experiment worktree, install pinned dependencies and prepare the normal generated clients/gallery. Browser support libraries on this machine are in `/home/dev/.local/opt/ipp-browser-support/sysroot/usr/lib64`.

```sh
npm ci
node tests/integration/prepare.mjs
npm run build:react
IPP_PERFORMANCE_EXPERIMENT=1 node tools/build_browser.mjs render-expanded headless
node tools/build_gallery.mjs
cargo build -p ipp-protocol --example profile_updates --release --features performance-experiment --locked
IPP_PROFILE_MODES=0,1,2,2,1,0 taskset -c 22 target/release/examples/profile_updates > target/performance/native-profile.log
LD_LIBRARY_PATH=/home/dev/.local/opt/ipp-browser-support/sysroot/usr/lib64 IPP_PROFILE_MODES=0,1,2,2,1,0 node tools/profile_robot.mjs
LD_LIBRARY_PATH=/home/dev/.local/opt/ipp-browser-support/sysroot/usr/lib64 IPP_PROFILE_MODES=0,1,2,2,1,0 IPP_PROFILE_PLAYING=1 node tools/profile_robot.mjs
```

Run native and browser timing separately after builds finish. `taskset` is optional outside Linux; record the new machine/environment. The profile includes allocation/reallocation request counts and bytes, not frees, peak live memory, allocation stack traces or hardware performance counters. The browser hook is a local Host diagnostic interface available only when experimental WASM exports are present. It does not alter the generated production protocol.

## Validation and limitations

The original baseline's 30 property-animation tests pass. The safe and pointer variants pass those 30 tests and six skeleton-animation tests. `source_replacement_in_failed_batch_invalidates_binding` also fails in mode 0: its negative-scale operation is accepted where the test expects rejection. The failure was reproduced independently and retained in `target/performance/baseline-skeleton-failure.log`; it was not repaired or counted as a passing test. The inherited `WorldSimulationState::fault` field is also unused in this snapshot.

The complete held-pose robot experiment passes state and pixel equality across all six blocks. No production optimization has been accepted, merged, committed or pushed. The primary checkout's concurrent work remains separate. The moving-scene experiment also passes all six blocks. Final TypeScript checking, pinned formatting, `check_repo.py`, `check_workspace.py` (eight crate boundaries and 76 resolved feature graphs) and `git diff --check` pass. Scoped Clippy across core/protocol/WASM and all their targets passes with `dead_code` allowed because of the inherited unused `fault` field; all other warnings are denied. The optional allocator is selected by the experimental executables, so it does not conflict with tests that install their own allocator. A final expanded WASM build checks that arrangement. No full regression was run because neither merge nor push was requested.

Validation commands and logs are retained under `target/performance/`; the Beads handoff records exact scope. Inherited source was formatted in the copied worktree, including one equivalent collapsed conditional for Clippy; the primary checkout was not changed by this experiment.

The [joint, material and response follow-up](reuse.md) compares the previous safe allocation pass with reusable sampling, property commits and response encoding.
