# Blender stress measurements — 14 September 2026

Historical measurements from the experiment worktree. For current maintained commands and opt-in scope, see the [benchmark guide](stress.md).

The [reusable benchmark](stress.md) was built in `/home/dev/ipp-worktrees/stress-benchmark`, branch `codex/stress-benchmark`, on `f524deb` with the preceding allocation improvements integrated. Measurements use Rust 1.98.1, Node 22.22.2, Blender 5.2.1 LTS and Chromium 153 with SwiftShader on the shared Ryzen 9 5900X development host. The historical browser sections below measure software-renderer Host tick times, not hardware GPU performance or display FPS. The follow-up [native hardware measurements](native-results.md) use an actual AMD Radeon RX 9070 XT and diagnose the remaining quadratic skinning invalidation.

## Authoring and import

The full fixture has 10,000 independently baked rigid cubes, 20 parented animated local lights, four walking Rigify humans, an animated camera, baked/native cube particles and deep attachment chains. The ten-second bake contains 241 samples. Bullet's mean cube drop is 5.9672 m; the compact exported rig matches its 222-bone Rigify source within a maximum matrix-element error of `9.835e-7`. Four rigs share the 31-joint pose asset.

Full generation took 79.47 seconds. Export produced 10,065 authored entities, 10,030 clip associations and 10,035 source assets with zero diagnostics in 344.29 seconds. Standard import produced 10,066 runtime entities, including the adapter's view camera, and a 3,201,782-byte saved World in 58.99 seconds. The 40,282 scene operations were acknowledged across ten bounded frames in approximately 0.54 seconds; total import includes asset conversion/publication and persistence.

| Measured operation                               |    Before |    After |
| ------------------------------------------------ | --------: | -------: |
| Small fixture animation export                   |  44.714 s |  7.083 s |
| Small clips-only import                          |  29.835 s |  2.611 s |
| Representative cube clip decoded storage, native | 246,376 B | 54,600 B |

The exporter now evaluates a shared action frame once for eligible objects, accumulates packed numeric columns and reuses the active action result for its library association. Drivers, NLA and different frame ranges retain isolated fallbacks. Small-scene active playback clip bytes match exactly. Library comparisons cover 1,028,195 numeric values; the maximum difference is `1.1920929e-7`, caused by the isolated path's temporary action/basis reassignment. The maintained real Blender equivalence test additionally checks restoration and NLA fallback.

Clips-only import now avoids creating and then deleting every animation controller. Binary HTTP publication replaces Playwright's conversion of clip bytes into JavaScript number arrays. Large revisions are split by actual receiving-client encoded size and the 4,096-operation limit, with alias remapping and partial-failure ordering preserved. The benchmark defers camera activation while importing and saving, avoiding expensive previews between multipart save chunks.

Typed key conversion could retain a generic enum buffer: the representative 241-key scalar track had capacity 1,280 and its rotation track capacity 768. Compaction at loading reduces both to 241 without changing encoded bytes or samples. The memory regression test enforces a resident-size budget and checks interpolation/round-trip identity. This measures owned clip storage; WASM memory capacity can remain substantially above live payload size.

## Frame comparisons

Mode 4 includes the previous joint/property/response reuse. Mode 6 adds indexed animation callbacks, cached description demand and reusable light preparation. Mode 7 also retains CPU-mesh demand and native particle simulation scratch. The full scene uses 157 controllers and 40,092 drivers, with 64 clip associations per controller. Both modes use the same compact loader and address fixes in these runs.

| Held workload | Mode 4 | Mode 6 |
| --- | --: | --: |
| 64 cubes: median Host tick | 27.7 ms | 26.4–26.8 ms |
| 64 cubes: Rust allocation calls/frame | 1,153 | 62 |
| 64 cubes: requested bytes/frame | 784,140 | 89,897 |
| 10,000 cubes, culling disabled: median Host tick | 31,037 ms | 3,675–3,677 ms |
| 10,000 cubes, culling disabled: Rust allocation calls/frame | 113,806 | 106 |
| 10,000 cubes, culling disabled: requested bytes/frame | 71,010,852 | 7,391,433 |

The full held frame improves approximately 8.4×. Small-scene timing changes are modest and overlap normal machine variation; the allocation reduction is repeatable. The small run uses 22 timed frames per block in order `6,4,6`; the expensive full run uses three timed frames per block in the same order and two separate allocation frames. These short full windows establish the large scaling problem, not a tight regression threshold for time.

Separate instrumented full-frame windows attribute approximately 21.80 seconds to animation in mode 4 and 1.23 seconds in mode 6. A property commit previously traversed unrelated controllers and drivers; the reverse target index limits callbacks to affected controllers. Description asset demand is retained until its inputs change. Controllers awaiting clips avoid rebuilding baselines for the unavailable tail on every frame. Validation, commit observers, invalidation and ordered restoration remain active.

Lighting previously built per-entity groups, ranked candidate vectors, prepared-light maps, shadow score maps and camera data repeatedly. It now retains those containers, aggregates bounds directly, prepares the camera once and queries scalar mesh bounds without allocating a compound geometry. The small frame has 94 draws and 2,496 triangles; the full unculled frame has 10,030 draws and 122,808 triangles, with no failed draws or unshadowed requested lights in the recorded held frame.

All measured held Rust allocations remaining in mode 6 are in rendering. The full split is two calls/182 B in core render preparation, five calls/2,600,480 B in GL frame preparation, and 99 calls/4,790,771 B in GL drawing. Animation, skeleton evaluation, skinning, hierarchy, asset dependency evaluation and the other non-render core System categories allocate zero in those held windows. This is not a claim of allocation-free moving scenes, JavaScript, transport or graphics drivers.

## Culling and the final allocation sweep

The benchmark now compares explicit mesh-derived bounds with the original imported scene. Bounds are attached through ordinary client operations; particles keep their separate conservative bounds rules. This exercises the existing culling policy and leaves an unculled control available.

The first full culling comparison reduced main submissions from 10,030 to 8,562 and shadow submissions from 40,120 to 42. Held Host tick time fell from 3,774.5 ms to 1,804.5 ms with exactly identical pixels. This exposed another allocation pathology: bounds bookkeeping copied a shared mesh URI for every mesh instance on every frame. Mode 6 consequently made 10,087 allocation requests per culled frame, including 10,030 URI allocations in asset dependency evaluation.

Mode 7 retains distinct source membership, uses borrowed comparisons and only updates service demand when membership changes. Native particle evaluation now also retains its chronological death heap and preserves scratch capacity on explicit restart. Its warmed small-scene core allocation rate falls from about 2.7 requests/frame to zero in the recorded moving windows. Existing birth ordering, split-step capacity admission, drain and restart tests pass. The latest small run has 54 allocation requests/65,417 B per culled held frame and zero non-render core allocations in both held and moving windows. Its moving renderer averages about 126 requests/233 KB per frame; particle visibility is independently proven by a fixed-camera restart comparison.

The final full held comparison records:

| Full held workload | Median Host tick | Rust calls/frame | Requested bytes/frame | Main draws | Shadow draws |
| --- | --: | --: | --: | --: | --: |
| Mode 7, original unculled import | 3,743.7 ms | 106 | 7,391,529 | 10,030 | 40,120 |
| Mode 6, explicit bounds | 1,735.9 ms | 10,087 | 4,094,894 | 8,562 | 42 |
| Mode 7, explicit bounds and retained demand | 1,802.0 ms | 53 | 3,181,577 | 8,562 | 42 |

Each full row uses five timed frames and two separately instrumented allocation frames. Culling accounts for the large timing gain; the mode 6/7 timing difference is within the variation of these short shared-host windows. Retained demand removes 10,034 allocation requests and 913,317 requested bytes per culled frame. All 53 remaining calls are in rendering: two/182 B in core render preparation, five/2,600,480 B in GL frame preparation and 46/580,915 B in drawing. Compared with the earlier mode 4 unculled control, the combined optimized/culling workload is about 17.2× faster and requests 95.5% fewer bytes. This combined comparison changes both runtime implementation and explicit scene bounds; the table separates their effects.

The final full moving window has a median Host tick of 1,776.7 ms, 142 Rust allocation requests and 4,997,459 B requested per frame across five separately instrumented frames. Its non-render core categories are also allocation-free. Render preparation accounts for two requests/182 B, GL frame preparation for 22/2,888,452 B and drawing for 118/about 2,108,825 B. Moving counts vary with live particles and camera visibility; they are not a fixed-state A/B comparison. The native-emitter restart removes 6,696 triangles and changes 22,799 pixels in a fixed camera. Changing only the isolated rig pose changes 18,729 pixels.

## Scale failures and remaining work

- A full runtime load exposed signed JavaScript interpretations of WASM addresses above 2 GiB. Input/output, asset delivery, diagnostic and instance-buffer address conversions now use unsigned offsets. The earlier full load and first render crossed that boundary successfully after the fix. The final moving run reserves 2,048.625 MiB of linear memory; retained capacity and loading fragmentation deserve a separate memory pass.
- The original full import exceeded the command count bound. Increasing queue capacity alone would not fix that wire-level limit; ordered framing is now handled by the adapter.
- The old full update path starved the client's maximum 60-second request deadline. Full mode 4 timings and allocation counters are saved before observation requests. Its full-frame pixel equivalence is not asserted; the small scene verifies exact state and pixels across modes, and the optimized full scene is checked independently against Blender.
- Ordinary Blender imports still omit `BoundingGeometry`; the benchmark explicitly adds it for its culling variant. Enabling appropriate bounds in authored scenes has a major shadow-submission benefit. The exporter does not silently change culling policy for every imported asset.
- Mode 8 now eliminates the warmed Rust allocations from renderer item snapshots, draw ordering/sort storage, shadow caster lists and particle instance vectors, as measured below. Ordinary shared cubes still use separate submissions; grouping compatible mesh/material/light configurations remains a larger renderer opportunity. Custom-material/error paths and JavaScript/driver allocations are outside the scene-wide zero-allocation claim.
- Mode 9 additionally indexes bound drivers within each affected controller. Native profiling then exposed repeated whole-World skin palette invalidation; mode 10 skips redundant scans while palettes are already invalid. See the native report. These measurements do not establish a benefit from unsafe writes; per-component commit dispatch remains a CPU cost.
- Loading 10,035 assets still takes about 60 seconds in this browser setup. Only about 165 MB is encoded animation data, but the bundle retains source JSON alongside encoded clips and performs many small requests. Measure request cadence, loading representations and existing archive providers before changing asset ownership or transport.

The separate small-scene JavaScript allocation sample still identifies `drawMesh`, instance upload, lighting upload and shadow binding as allocation sites. It also includes substantial profiler/DevTools serialization, so its total is not presented as application bytes per frame.

## Evidence

The full Blender files and portable World bundle are under `target/stress-benchmark/full/`; the maintained small fixture is under `target/stress-benchmark/validated-smoke/`. Small comparison, native particle and isolated Rigify evidence is in `target/integration-artifacts/stress/ipp-browser-blender-stress-benchmark-kxE9CY/`. Its fixed-camera pose change affects 19,150 pixels. Full unculled comparison measurements are in `target/integration-artifacts/stress/ipp-browser-blender-stress-benchmark-KHAGwh/`; its rendering/state assertions passed, but its final worker shutdown exceeded the former one-second close deadline, so that run is not a complete passing test. The transport now gives graceful shutdown the configured deadline and has real MessageChannel coverage for delayed completion and timeout disposal. The first full culling measurements are in `target/integration-artifacts/stress/ipp-browser-blender-stress-benchmark-nsa1lq/`; that run exposed a flawed particle assertion comparing total triangles across moving cameras, replaced by the fixed-camera restart check. The latest complete small run is `target/integration-artifacts/stress/ipp-browser-blender-stress-benchmark-p8A86V/`. The final full run is `target/integration-artifacts/stress/ipp-browser-blender-stress-benchmark-oqquPb/`; all held/culling, Blender timeline, native particle, isolated rig and clean shutdown checks pass. Its matching saved World was freshly reimported after the private particle scratch layout changed. The full browser scenario completed in 577.85 seconds.

Focused checks include core animation/lifecycle/assets/geometry tests, GL/Host/protocol/WASM tests, the actual Blender disk-import and lighting browser scenarios, the existing Blender exporter tests and the new action-sampling equivalence test. The clip compaction checkpoint passes 31 animation, ten dynamic-property and seven skeleton-animation tests. The additional moving-allocation budget is verified by the retained full profile and a fresh complete small run in `target/integration-artifacts/stress/ipp-browser-blender-stress-benchmark-5rGw6i/`. Final validation additionally passes the five particle simulation/cache tests, scoped Clippy with warnings denied, TypeScript checking, repository/workspace checks and the pinned formatting checks. No merge, push, commit or full regression pass was requested.

## Retained renderer buffers: mode 8

The renderer now borrows prepared World items and owns growing scratch for draw ordering, shadow caster indices, particle instances, light candidates and debug membership. It borrows the active camera component and caches private sprite metadata. Scratch returns to the service on success or failure and retains no World/GPU borrows. An original-index tie breaker preserves the previous stable ordering without allocating sort storage.

| Warmed workload | Mode 7 calls / bytes per frame | Mode 8 calls / bytes per frame | Mode 7 → 8 median tick |
| --- | --: | --: | --: |
| 64 cubes, unculled | 57 / 71,801 | 0 / 0 | 27.6 → 27.1 ms |
| 64 cubes, culled | 49 / 59,513 | 0 / 0 | 11.7 → 10.9 ms |
| 10,000 cubes, unculled | 101 / 4,970,073 | 0 / 0 | 3,658.4 → 3,628.3 ms |
| 10,000 cubes, culled | 50 / 2,845,017 | 0 / 0 | 1,745.9 → 1,806.1 ms |

Both small and full moving windows also make zero Rust allocation/reallocation requests after warmup; medians are 11.6 ms and 1,748.4 ms respectively. This is allocation elimination with no convincing timing improvement in these software-renderer windows. The current control also uses compact draw/caster indices, so its allocations differ from historical mode 7 records above.

Exact mode/culling/memory-growth pixels, timeline samples, isolated native particles and Rigify deformation pass at both scales. Evidence: `target/integration-artifacts/stress/ipp-browser-blender-stress-benchmark-9f53bT/` (small) and `...-9hWbtC/` (full), with analyses/logs under `target/render-buffer-reuse/`. These are instrumented WASM `release-small` builds using Chromium SwiftShader. They prompted the separate native hardware investigation; they must not be quoted as accelerated GL performance.
