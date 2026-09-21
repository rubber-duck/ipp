# Joint, material and response allocation follow-up

Historical measurements from the experiment worktree. For current maintained commands and opt-in scope, see the [benchmark guide](stress.md).

This extends the [allocation sweep](allocations.md) in `codex/performance-experiments`, worktree `/home/dev/ipp-worktrees/performance-experiments`, Beads `ipp-b3l`. Mode 3 is the previous allocation optimization; mode 4 additionally reuses joint, numeric property and response storage. Ordinary builds enable the new paths. The earlier callback-bypassing pointer prototypes remain separate controls, not implementations of these changes.

## Changes and ownership

- Skeleton runtime data owns an evaluation buffer and sampled-joint flags, excluded from authored copies and serialization. Sampling borrows a key segment once, evaluates each joint into scratch, validates the complete contribution and then publishes it. Sparse driver originals retain their capacity. Sparse overrides are decoded through a validated borrowed iterator. This removes current/sample/reference/result pose vectors and the per-frame map/set of sampled joints while preserving discrete pose rebasing.
- Eligible existing numeric properties use `SystemRuntimeAccess::apply_evaluated_properties`. Animation stages the affected numeric lanes, and CustomMaterial validates their identities, types and finite values without copying its source string, descriptors, asset references or unrelated property bytes. Every selected validator and commit observer still runs; old storage remains live until publication. Whole-value updates remain for discrete/resource/descriptor changes. Mixed numeric and whole-value edits to the same component preserve declaration order. The same path handles ParticlePlayback time: the robot exposed six more source-string copies per sample through that clock field. Particle component-presence validation now reads metadata without cloning values. Resource-free numeric tracks also avoid rebuilding evaluated resource demand.
- `encode_response_into` clears and writes an exclusively owned growing buffer, retaining capacity on errors and exposing no partial message. Host response storage starts at 4 KiB when the pool is empty and grows for larger payloads. Queued and borrowed responses keep their own buffers; the WASM boundary returns storage only at the next ABI call that invalidates the previous output. Separate pending responses cannot overwrite one another. Recycled bytes carry no session identity. The free-buffer pool reserves slots for session queues and in-flight output; surplus returned control/provider buffers are dropped when those slots are full, avoiding growth with lifetime request count. The existing protocol size and queue limits still apply.

The Host API also exposes explicit recycling for other consumers. A consumer that takes an owned `Vec` and never returns it does not gain steady-state reuse; the native WebSocket adapter currently transfers ownership to Tungstenite and is such a consumer. This pass measures the actual browser/WASM path and the reusable encoder, not allocation-free native WebSocket delivery. JS transferred response buffers remain separate allocations.

## Validation and measurement

Native measurements exercise uploaded decoded clips and real World preparation, asset polling and evaluation, with additive weighted drivers on 1/64 material, skeleton or particle-cache instances. Each material includes 32 unrelated matrix properties. The response workload tests both a 33-byte Frame and an 8 KiB error payload. Warmed mode 4 asserts zero allocator calls and requested bytes. Allocation windows are separate from timing windows; release timings use CPU 22 and opposite-order blocks `3,4,4,3` on the shared Ryzen 9 5900X host. Joint output, material values and actual cached particle positions are checked outside measured windows. Requested bytes measure allocation traffic, not retained heap.

The real robot uses the maintained gallery, generated client, worker transport, WASM, assets and WebGL renderer. Held comparisons assert identical entity/controller state and zero changed pixels; walking comparisons assert advancing playback and changed pixels. Captures after forced WASM growth also check cached-view refresh. Software rendering and shared-host drift limit small timing claims.

The maintained browser profiling entry now runs the Platformer gallery workload because Platformer replaced the robot gallery page. The retired robot sources and assets have been removed. The robot measurements and commands recorded below are historical evidence from the original workload and are not directly comparable with new Platformer profiles.

The private Skeleton layout change changes the generated target contract. The saved robot World was regenerated from the checked-in Blender disk export using `tools/import_blender_scene.mjs --namespace clunker --world clunker.ipp --clips-only`. Its 69 entities, 10 exported clips, manifest, catalog and content-addressed assets are unchanged. The target contract and capture metadata are regenerated through the runtime, with no patched headers or compatibility bypass.

## Native results

Each row includes normal World preparation, asset polling and evaluation unless identified as encoding only. Times are microseconds per update, medians of two opposite-order blocks. “Before” is mode 3; “after” is mode 4.

| Workload | Time before → after, µs | Allocations before → after | Requested bytes before → after |
| --- | --: | --: | --: |
| 1 material, animated dynamic value | 8.273 → 3.788 | 159 → **0** | 18,564 → **0** |
| 64 materials, animated dynamic value | 595.976 → 338.489 | 9,987 → **0** | 1,140,972 → **0** |
| 1 material, animated alpha cutoff | 8.041 → 3.277 | 159 → **0** | 18,564 → **0** |
| 64 materials, animated alpha cutoff | 520.194 → 207.477 | 9,987 → **0** | 1,140,972 → **0** |
| 1 skeleton, 2 animated joints | 2.801 → 2.551 | 9 → **0** | 1,155 → **0** |
| 64 skeletons, 2 animated joints each | 72.725 → 57.015 | 522 → **0** | 54,144 → **0** |
| 1 particle-cache playback | 3.812 → 3.450 | 9 → **0** | 820 → **0** |
| 64 particle-cache playbacks | 240.351 → 225.908 | 387 → **0** | 5,356 → **0** |
| 33-byte Frame response encoding | 0.1063 → 0.0100 | 4 → **0** | 120 → **0** |
| 8 KiB payload response encoding | 0.1649 → 0.0942 | 4 → **0** | 8,279 → **0** |

At 64 instances, dynamic material updates take 43.2% less time, material alpha updates 60.1% less, joint updates 21.6% less and particle-cache playback 6.0% less in this fixture. These are fixture measurements, not scene FPS gains. The existing scalar fixture also remains at zero calls/bytes for all 32 sampled mode-3/mode-4 frame blocks, covering idle, one-controller and many-controller cases through 512 scalar drivers.

Evidence: `target/allocation-followup/native-final.csv`, `native-summary.json`, `scalar-final.log`; both native error logs are empty.

## Robot results

The final held-pose comparison removes all measured animation and Frame-response allocation categories. The remaining 42 allocations/frame belong to rendering. All four blocks retain exact entity/controller state and identical captured pixels, including the captures after forced WASM growth.

| Held robot metric      | Mode 3 |     Mode 4 |
| ---------------------- | -----: | ---------: |
| Timed frames           |    202 |        203 |
| Profiled frames        |     43 |         43 |
| Rust allocations/frame |    204 |     **42** |
| Requested bytes/frame  | 56,534 | **35,513** |
| Mean Host tick, ms     | 21.231 |     21.301 |
| Median Host tick, ms   | 20.400 |     20.200 |
| p95 Host tick, ms      | 29.700 |     30.100 |

This follow-up removes **162 allocations/frame (79.4%)** and **21,021 requested bytes/frame (37.2%)** from the held robot. It shows **no meaningful scene-time improvement**: mean tick time increases by 0.3%, while the median decreases by 1.0%. These software-rendered/shared-host timings are effectively unchanged. They measure Host work, including graphics submission/waits, and exclude the timer gap; they are not FPS measurements.

| Remaining exclusive held-frame category | Calls/frame | Requested bytes/frame |
| --- | --: | --: |
| RenderSystem evaluation | 3 | 276 |
| GL render preparation | 9 | 18,367 |
| GL draws | 30 | 16,870 |
| Total | **42** | **35,513** |

Held timing block means in order 3/4/4/3: 21.302 / 21.384 / 21.220 / 21.160 ms. The baseline held particle population differs from the earlier report's paused pose, so compare modes within this run; do not subtract the old 200-call result from the new 42-call result.

Held evidence: `target/integration-artifacts/gallery-combined/ipp-browser-robot-performance-NIxo3y/profile.json`; completed captures, environment and cleanup records accompany it. `target/allocation-followup/robot-summary.json` aggregates counts and categories.

| Walking robot metric   | Mode 3 |     Mode 4 |
| ---------------------- | -----: | ---------: |
| Timed frames           |    103 |        102 |
| Profiled frames        |     42 |         43 |
| Rust allocations/frame |  302.2 |  **128.2** |
| Requested bytes/frame  | 63,473 | **41,006** |
| Mean Host tick, ms     | 21.178 |     21.576 |
| Median Host tick, ms   | 20.200 |     20.800 |
| p95 Host tick, ms      | 30.200 |     26.600 |

Walking allocations fall by **57.6%**, requested bytes by **35.4%**. Animation remains at zero in every measured mode-4 category. Unlike the held pose, moving playback performs additional Host/resource work: mode 4 averages 79.2 calls/frame under Host work, 6.9 under asset polling/reconciliation and 42.1 under render preparation/draws. These eventful frames are not allocation-free, even though the recurring Frame encoder reuses its storage.

Walking shows no reliable scene-speed improvement either: the mean increases 1.9%, median increases 3.0% and p95 decreases 11.9%. Block means in order 3/4/4/3 are 21.151 / 21.188 / 21.965 / 21.204 ms. Workload transitions occur at different wall-clock times, so the walking allocation difference also includes slightly different event/particle activity. Playback advances, rendered pixels change, and both mode-4 captures survive memory growth.

Walking evidence: `/home/dev/ipp-worktrees/performance-experiments/target/integration-artifacts/gallery-combined/ipp-browser-robot-performance-6EMDaO/profile.json`. Both final runs include 69 entities, 8 controllers and 41 declared drivers. The complete per-category data is in `robot-summary.json`; JavaScript and GPU allocation traffic are not included.

## Remaining costs and scope

The changes remove allocation traffic in the measured animation and response paths after warmup. They do not make every IPP frame allocation-free. Render snapshots, sorting/shadow collections, custom-material preparation and particle instance vectors still allocate. Resource transitions, live particle emission, active constraints, debug rendering and owned command outcomes have additional allocations outside the fixed native fixtures. Command vector capacity is reusable, but command payloads, outcomes and WASM ingress reservations still contain owned storage; the complete active command/response pipeline has not been measured at zero. Native WebSocket ownership and JS transfer buffers are also outside the reuse result above.

No new unsafe assignment or third-party runtime dependency is needed for these improvements. Commit validation, observers and invalidation still run. Their repeated controller/binding traversal remains a CPU scaling cost; removing allocations does not remove those indirections. The earlier safe/raw-pointer experiment and its limitations remain documented in the [initial report](README.md).

## Reproduce

```sh
cargo build -p ipp-protocol --example profile_allocations --example profile_updates --release --features performance-experiment,skeletal-animation,particles --locked
taskset -c 22 target/release/examples/profile_allocations
IPP_PROFILE_MODES=3,4,4,3 taskset -c 22 target/release/examples/profile_updates
IPP_PERFORMANCE_EXPERIMENT=1 node tools/build_browser.mjs render-expanded headless
node tools/build_gallery.mjs
LD_LIBRARY_PATH=/home/dev/.local/opt/ipp-browser-support/sysroot/usr/lib64 IPP_PROFILE_MODES=3,4,4,3 node tools/profile_robot.mjs
LD_LIBRARY_PATH=/home/dev/.local/opt/ipp-browser-support/sysroot/usr/lib64 IPP_PROFILE_MODES=3,4,4,3 IPP_PROFILE_PLAYING=1 node tools/profile_robot.mjs
```

Finish builds before profiling; run native and browser timing separately. Browser environment: Chromium 153, SwiftShader/WebGL 2, optimized `release-small` WASM on the shared Ryzen 9 5900X host. Rust 1.98.1, Node 22.22.2. JavaScript heap sampling was not repeated in this follow-up; the previous report's JS estimates remain historical evidence.

## Validation and handoff

- Expanded library checks: 129 tests pass across core (45), HostSession (24), protocol (37), renderer (11) and WASM (12). This includes joint interpolation/additive equivalence and stable output storage; material field validation; response encoding growth/error recovery; multiple outstanding responses, buffer ownership, bounded free-pool growth and session changes; actual WASM Frame buffer reuse.
- Focused core integration checks: 86 tests pass across animation, assets, dynamic properties, evaluated properties, skeletons and overlays. One inherited `source_replacement_in_failed_batch_invalidates_binding` case remains skipped; the earlier pass reproduced its negative-scale expectation failure with mode 0. It is not passing coverage. The evaluated-property test checks duplicate fields, before/after observer values, retained identity and invalid numeric/type/key/resource rejection.
- Real maintained `custom-materials` and `crt-particles` browser suites pass. They exercise generated-client transport, assets, rendering assertions, sparse overlays, fallback/context recovery, robot cached particles, pause and exact rewind.
- Scoped all-target Clippy passes for core, protocol, HostSession, renderer and WASM with expanded capabilities and the experiment feature, using `-D warnings -A dead-code`. The allow is for the inherited unused `WorldSimulationState::fault`. Matching expanded/minimal WASM distributions build successfully.

Pinned source formatting/checking, changed-report Prettier checks, TypeScript checking, repository/link checks, workspace policy (eight crates/76 resolved feature graphs) and whitespace checks pass. Final held and moving robot runs pass all four blocks each and clean up their browser, workers and server. Evidence is under `target/allocation-followup/`: `lib-final.log`, `response-final-tests.log`, `focused-final.log`, `browser-suites.log`, `clippy-final.log`, `browser-final-build.log`, `robot-held-measured.log`, `robot-moving-measured.log`, `format-handoff.log`, `format-final-check.log`, `report-format.log`, `typescript-final.log`, `check-repo.log`, `check-workspace.log`, `diff-check.log`, and `final-build-sha256.txt`. `followup-only.patch` and `followup-manifest.json` separate these changes from the previous pass. A verified complete source/evidence archive is retained outside the worktree at `/home/dev/ipp-worktrees/performance-evidence/allocation-followup-20260914/`. The work remains uncommitted in the experiment worktree for integration review against concurrent primary-checkout changes. No merge, push or full regression was requested or performed.
