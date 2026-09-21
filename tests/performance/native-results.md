# Native evaluation and GPU benchmark — 14–15 September 2026

Historical measurements from the experiment worktree. For current maintained commands and opt-in scope, see the [benchmark guide](stress.md).

The earlier 1.7–1.8 second culled frames were instrumented WASM on Chromium **SwiftShader**, a software renderer. They were not accelerated GL timings. The native follow-up uses real hardware: **AMD Radeon RX 9070 XT**, `radeonsi / gfx1201 / ACO`, OpenGL ES 3.2, Mesa 26.1.8, on the shared Ryzen 9 5900X host. Rust is 1.98.1. See the [reusable commands and measurement contract](stress.md#run).

## Overview versus interior camera views

The `--culling-views` benchmark now compares the existing animated overview with an interior camera at `[0, 2.2, 0]`, pitched ten degrees down, looking forward and turned ninety degrees. These first three views retain the original 35.05-degree vertical field of view, 0.1 m near plane and 1,000 m far plane. A fourth view explicitly reduces the far plane to 30 m. Each station reloads the same saved World and renderer, so live particles reset along with animation. All 40,092 original animation drivers, including the unused overview camera's drivers, remain active. One nonrendering probe camera is added to each World.

Two ordinary-release runs on the same RX 9070 XT / Ryzen 5900X hardware, 800×600, 200 advancing warm-up frames and 60 measured frames per view:

| View | Median surface / shadow draws | Render preparation/submission median | Complete frame median | Complete frame p95 |
| --- | --: | --: | --: | --: |
| Overview | 8,569 / 43 | 6.017–6.305 ms | 10.395–10.861 ms | 15.527–16.127 ms |
| Interior, original far plane | 1,110 / 8 | 3.044–3.370 ms | **7.474–7.672 ms** | 12.615–13.614 ms |
| Interior, turned 90 degrees | 1,136 / 29 | 3.327–3.430 ms | 7.609–8.324 ms | 12.547–13.640 ms |
| Interior, 30 m far plane | 272 / 8 | 2.752–3.162 ms | 6.992–8.991 ms | 11.918–15.202 ms |

Camera placement alone reduces surface draws by about 87% relative to the overview. At the final interior pose, the camera query admits 1,108 of 10,032 renderable entities, rejecting about 89%; the 30 m query admits 269, rejecting about 97%. Candidate entities differ slightly from surface draws because particle groups and per-instance clipping have their own submission rules. All bounds are known in these captures. These results use the automatic bounds and fresh-state harness; they are a new regression baseline, not evidence of another engine optimization since the preceding pass. Shared-host variation is visible, particularly in the second 30 m update window: fewer draws do not imply a lower observed total in every run. Offscreen objects still animate and update geometry.

Retained single-camera query timings, measured separately at the completed pose, expose the spatial structure's tradeoff:

| View             |         BVH median | Exhaustive flat scan median |
| ---------------- | -----------------: | --------------------------: |
| Overview         |     0.534–0.536 ms |          **0.284–0.285 ms** |
| Interior         | **0.083–0.088 ms** |              0.127–0.132 ms |
| Interior, turned | **0.084–0.085 ms** |              0.125–0.126 ms |
| Interior, 30 m   | **0.032–0.034 ms** |              0.125–0.127 ms |

The overview admits most objects, so tree traversal adds work compared with a flat scan. The interior views prune enough of the tree to benefit. These query measurements include retained result publication, but exclude geometry evaluation/refitting and the renderer's additional light/shadow queries. Both backends perform frustum rejection; this is not a culling-disabled render or an occlusion test. Cubes behind other cubes remain candidates if they overlap the camera frustum.

The code inspection also identifies remaining render preparation work: `select_prepared_object` still runs for offscreen light receivers, and material/depth ordering still processes all render items before submission rejects invisible ones. The camera query itself is a small part of the interior frame. Filtering that preparation earlier is a concrete follow-up opportunity, preserving light-selection history and independently visible shadow casters. No renderer semantics were changed in this benchmark pass.

Every view has identical BVH/flat candidate identities and completed captures, and every rejected entity is checked against the separate geometry-visibility path. Captures repeat byte-for-byte across both ordinary runs. All twelve Blender animation probes and teardown pass in each run. The separately instrumented run passes all eight warmed allocation windows with **zero Rust allocation calls and requested bytes**, across five counted frames each; setup, loading, structural changes, JavaScript and driver allocations remain excluded.

Focused validation includes expanded example Clippy, native release and instrumented builds, the three complete camera-comparison runs, pinned formatting, repository and whitespace checks. The initial exploratory run reused live particle state between camera stations; it remains under `first` for diagnosis and is excluded from the final tables. No full regression, commit, merge or push was requested.

Artifacts: `target/culling-views/validated`, `repeat`, `allocations`, their matching native hosts, `timing-summary.json`, `query-summary.json`, `image-comparison.json`, `allocation-summary.json`, and `overview-and-interior.png`. See [reproduction and measurement scope](stress.md#interior-camera-views).

## Retained render inputs, upload tracking and material ordering

This pass implements all four agreed changes. Opaque/cutout draws group by shader and material values, then sort front-to-back inside each group. Blended draws retain back-to-front ordering and particle instances retain their submission groups. Material fingerprints affect ordering only; upload decisions compare actual values.

Downstream transform consumers borrow the cached matrix/inverse pair. Geometry compares the borrowed matrix before copying a changed placement into its retained exact query shape. Ordinary render records update occupied slots instead of rebuilding and copying their static fields each frame; lifecycle changes rebuild templates. Normal matrices are derived from the existing inverse when the retained render transform changes and are borrowed during drawing. Exact geometry output still owns its placement, and particle records still carry their individual transforms.

GLES and WebGL now retain exact lighting/shadow uniform values per program, skip unchanged uploads, upload only active light slots, and retain the shadow atlas binding within submission boundaries. Light preparation no longer clears inactive buffer tails. Context, program and resource lifetime boundaries invalidate the relevant caches. These changes do not introduce a depth prepass or new ordinary-object instancing.

Ordinary native release, accelerated RX 9070 XT GLES at 800×600, the same saved 10,066-entity scene, 60 advancing frames per window. The first control was captured before implementation; the following optimized/control/optimized runs were sequential, without task-owned builds or browser tests competing for timing:

| Median per frame | Two preserved control runs | Two optimized runs |
| --- | --: | --: |
| Complete moving frame | 15.281–15.820 ms | **11.668–14.052 ms** |
| Render preparation/submission | 9.566–10.013 ms | **6.406–8.230 ms** |
| Nonrender moving update, isolated window | 4.528–4.585 ms | **4.474–4.879 ms** |
| Later held pose-animation update | 3.046–3.059 ms | **2.937–2.945 ms** |

The corresponding complete-frame improvements are 8% and 26%; render preparation/submission improves 14% and 36%. The isolated moving update has no clear improvement. Normal preparation now occurs in the core render-input stage instead of during GL submission, so the changes also move some work between those stages. Complete-frame p95 is 16.614–18.984 ms after the change, versus 19.942–20.557 ms in the controls. These short, repeated windows expose shared-host variation; the median does not establish a guaranteed frame budget. Stage medians need not sum to the total median, and render submission can include GL driver stalls. The approximately 0.2 ms completion wait measures only the final wait, not total GPU execution.

The separate 256-draw mixed custom-material fixture, measured over 120 frames with alternating builds, remains approximately 0.85 ms total: 0.822–0.866 ms control versus 0.863–0.866 ms optimized. Its render stage is 0.516–0.550 ms control versus 0.550–0.554 ms optimized. This fixture establishes no speedup; numeric/resource transitions, stop restoration and captures still pass. The full scene continues to issue 8,583 surface and 35 shadow draws in its moving window.

All six full-scene and four mixed-scene warmed allocation windows report **zero Rust allocation calls and requested bytes**, across five counted frames each. This covers ordinary warmed evaluation and rendering, not initial growth, loading, structural edits, JavaScript or graphics-driver internals.

The full-scene captures repeat exactly within each build. Each held capture differs from the original at one channel of one pixel by one intensity level. Moving captures differ at 86 of 480,000 pixels after opaque reordering. A diagnostic build retains all other optimizations but restores the original opaque order: its moving capture is byte-identical to the control, isolating those changes to ordering. Inspection shows tiny cube-edge coverage changes, consistent with different winners at equal quantized depth. The diagnostic does not replace the selected production order and is not used for timing claims. All twelve independent Blender position/rotation probes pass. Mixed-scene captures are byte-identical before/after.

Validation: 90 focused core integration tests; 22 GL unit tests; rigid-bounds and cached-normal tests including shear, reflection and extreme scales; 33 maintained scenarios through native WebSocket, generated clients, worker/WASM and completed WebGL frames; the hardware GLES smoke suite; expanded, minimal and particles-without-shadows Clippy; pinned formatting, repository and whitespace checks. The custom-material browser scenario now exercises overlapping opaque objects and changed material values alongside its existing transparency and context-recovery assertions. Browser startup was repaired using the existing local dependency directory; the native texture smoke was rerun with its required UV/checker fixtures. No full regression, commit, merge or push was requested.

Evidence is under `target/material-order`: preserved `before` / `repeat-before`, optimized `after` / `repeat-after`, alternating `mixed-*` runs, `allocations`, `mixed-allocations`, matching native builds, CSVs, captures, `timing-summary.json`, `image-comparison.json`, `allocation-summary.json`, and the diagnostic `entity-order-*` files. `source-before` and `pass.patch` isolate this pass from inherited changes. Production source hashes match the measured optimized build after the diagnostic source was restored.

## Direct rigid bounds and retained replacement animation

The next pass removes routine restoration for controllers whose numeric drivers all replace their destinations and whose component targets have no competing controller. Eligibility is compiled with binding metadata. Empty mutation boundaries leave the preceding sample applied; the next sample writes its destinations once. Producer mutations, explicit controller withdrawal/restoration, unavailable bindings and failed time advancement still withdraw retained values. Mixed or overlapping composition retains its necessary baseline processing. These rules add one controller flag, without a component mirror or per-frame driver scan.

Joint restoration values now refresh during binding/input preparation rather than scanning every property driver each frame. Normal joint blending continues to use the Skeleton evaluator's prepared local pose. Initialized rigid geometry updates its retained exact shape placement and computes the world box directly; unchanged placements skip spatial publication. Picking and debug drawing keep the oriented shape, and lifecycle invalidation returns to normal preparation.

Ordinary native release, the same saved 10,066-entity scene, accelerated RX 9070 XT GLES at 800×600, 60 frames per window, with alternating preserved executables and no task-owned compilation during timing:

| Nonrender update median | Repeated original control | Repeated geometry-only change | Combined changes, three runs |
| --- | --: | --: | --: |
| Moving animation | 6.679 ms | 5.523 ms | **4.520–5.128 ms** |
| Later held pose-animation window | 4.516 ms | 3.648 ms | **3.047–3.142 ms** |

The combined moving update is 23–32% below the repeated original control. Its observed p95 is 9.269–10.482 ms. The first original run was noisy at 9.302 ms; it is retained in the artifacts and is not used to claim a larger improvement. Earlier geometry-only repetitions were 5.345 and 5.396 ms. Render submission was approximately 9.6–9.7 ms in the first two final runs; their complete moving-frame medians were 15.450 and 15.829 ms. A final rebuild after comment/local-name/test-format cleanup measured 10.470 ms rendering and 19.348 ms total despite its faster nonrender window, so this pass does not establish a rendering or complete-frame budget guarantee. Stage medians need not sum to the median total, and shared-host variability remains visible.

All eight ordinary runs have byte-identical held and moving captures, and pass twelve independent Blender position/rotation probes and teardown. All six instrumented warmed windows report zero Rust allocation calls and requested bytes across five counted frames each. Instrumented timings are separate from the ordinary-release table; allocation scope still excludes initial growth, structural edits, loading and graphics-driver internals.

A fresh 999 Hz CPU-clock trace uses the same 240 advancing frames plus five settling frames and excludes load/warm-up. Sampled main-thread CPU per frame changes as follows; these are attribution estimates from separate profiling builds, not the ordinary-release stage medians above:

| Sampled CPU work | Previous trace | Final trace |
| --- | --: | --: |
| Animation restoration | 1.050 ms | **0.012 ms** |
| All-driver skeleton-source scan | 0.351 ms | **No samples; steady scan removed** |
| Complete animation stage | 4.817 ms | **3.612 ms** |
| Complete geometry stage | 3.260 ms | **2.729 ms** |

The first two rows are contained in animation. Sampling/writing remains the dominant animation work. Geometry still retains exact oriented shapes for picking/debug consumers; index maintenance is 0.212 ms within its total. Main-thread renderer samples remain similar at 7.281 ms. Driver workers, GPU execution and blocked time are outside this attribution.

Validation passes 110 focused core integration tests plus two direct rigid-bounds unit tests, expanded and lean all-target Clippy, and 32 maintained animation/geometry/skinning scenarios through native WebSocket, generated clients, worker/WASM and completed WebGL captures. Added checks cover one numeric notification per ordinary frame, producer edits and stop, failed advancement, explicit controller restoration, changed authored joint poses, affine bounds and retained query storage. The initial unfiltered library run was stopped during an unrelated 16,000-entity debug scaling test; the focused selections above provide the completed evidence. Formatting, repository and whitespace checks pass. No full regression, commit, merge or push was requested.

Artifacts are under `target/rigid-bounds`: the original `before` / `repeat-before`, geometry-only `after` / `repeat-after` / `repeat-geometry`, combined `final` / `repeat-final` / `validated`, `allocations` / `validated-allocations`, preserved `native-host` (geometry only), `final-native-host`, `validated-native-host`, `validated-instrumented-host`, `frame-pointer-host`, and timing/image summaries. Current-source/executable hashes match for the three validated/profile hosts. `frame.data`, `all.folded`, `sampling-summary.json`, `frame-flamegraph.svg` and `renderer-flamegraph.svg` retain the final profile. The original executable remains `target/spatial-preparation/validated-native-host`; all use the existing full native input bundle. `source-before` and `pass.patch` isolate this pass from the inherited uncommitted experiment. Reuse the [native comparison commands](stress.md#draw-and-spatial-comparisons) with these host/output paths.

## Shared bounds, batched spatial queries and light selection

The follow-up implements automatic `BoundingGeometry` requirements, component-owned world AABB/sphere caches, and GeometrySystem-owned flat/BVH indexes. Camera and shadow queries share retained multiword bitmasks; rejected query bits disappear during descent, with the same path for a single bit. Unknown or unproven bounds remain candidates. Particle bounds enclose the evaluated effect. Ordinary radii are retained; overlap rejection squares the sum of radii.

Lighting reads prepared entity slots, rejects by squared range and algebraic spotlight overlap, and defers influence ranking until more than eight candidates survive. Shadow priority remains separate. Nonreceiving custom materials share a camera/ambient block. Retained vectors, masks, tree storage and draw blocks grow with demand. No automatic mesh instancing or shader changes are included.

Ordinary release on the same hardware and saved full scene, **60 frames per window**, with all task-owned compilation and other rendering stopped:

| Moving full scene | Preserved before | Final BVH | Final flat index | Before repeated afterward |
| --- | --: | --: | --: | --: |
| Render preparation/submission CPU | 15.103 ms | **9.640 ms** | 9.945 ms | 16.575 ms |
| Complete frame, including GPU completion | 20.962 ms | **16.673 ms** | 18.075 ms | 24.790 ms |
| Update only, including bounds/index maintenance | 4.153 ms | **6.327 ms** | 5.578 ms | 4.277 ms |
| Later pose-animation update only | 3.893 ms | 4.492 ms | 4.498 ms | 3.891 ms |

Rendering falls by 36–42% against the two controls. The cost moves partly into core geometry: update-only increases by approximately 2.1 ms with BVH. Its update-only p95 is 11.430 ms in this run, within the requested 15 ms nonrender budget. The complete moving-frame median improves by 20% against the first control. Shared-host variability is visible in both control runs; these measurements do not establish a universal BVH advantage. BVH pays maintenance that flat scanning avoids, so both remain available through the same query interface. Per-column medians need not sum to the median total.

The final 999 Hz `perf` trace covers 240 advancing frames plus five settling frames, excluding load/warm-up. Compared with the preserved previous trace:

| Sampled CPU work per frame | Before | Final |
| --- | --: | --: |
| Light preparation | 5.258 ms | **1.622 ms**, including 0.584 ms batched queries |
| Core geometry evaluation, including index maintenance | 1.475 ms | 3.260 ms |
| BVH maintenance within geometry | — | 0.143 ms |

Light preparation falls from about 40% to 22% of renderer samples. The new bounds cache initially performed duplicate affine copies and generic plane calculations; removing those reduced the first implementation's geometry cost. Geometry evaluation remains a useful future CPU target. Sampled costs are CPU attribution estimates from separate builds, not stage timers or GPU time.

All six full-scene and four mixed-scene warmed allocation windows report **zero Rust allocation calls and requested bytes** across five counted frames each. Initial growth, structural edits, loading/recovery, JavaScript and driver-internal allocations are outside this claim. The 256-draw mixed custom-material scene also passes actual texture transitions and stop restoration, with approximately 0.54 ms ordinary-release rendering CPU in the first final run.

Held and moving full-scene captures are **byte-identical before/after**; flat and BVH captures also match exactly. World AABBs conservatively admit a few more draws than the old oriented bounds, so draw counts need not match. The twelve independent Blender probes and teardown pass. Automatic bounds mean the original initial window is now `held-default-bounds`; the subsequent authored generated bounds preserve the image. The maintained geometry scenario separately verifies actual culling against deliberately unproven authored bounds.

Validation includes 119 initial focused core tests, followed by the new bounds/overlay cases, spatial refit/multiword tests and 17 GL unit tests; expanded and lean Clippy; five final WebGL tests covering custom materials, shared geometry, lighting/recovery and particles; and the complete hardware GLES smoke suite covering lifecycle removal, textures, streaming cancellation, normals, bounds and recovery. The native fixture helper now progresses resource lifecycle notifications through its Host before rendering. Pinned formatting, TypeScript and repository checks accompany the change. No full regression, commit, merge or push was requested.

Artifacts are under `target/spatial-preparation`: `before-validated`, `validated-bvh`, `validated-flat`, `repeat-before`, `validated-allocations`, `validated-mixed-allocations`, `validated-sampled`, matching `validated-*-host` builds, per-frame CSVs, `timing-summary.json`, `image-comparison.json`, `sampling-summary.json`, `frame-flamegraph.svg` and `renderer-flamegraph.svg`. The initial source snapshot and incremental pass diff are retained separately. See [reproduction and comparison scope](stress.md#draw-and-spatial-comparisons).

## Previous render preparation pass

The renderer now reuses evaluated bounds and camera visibility, precomputes per-light scoring constants and selected-light uniform/shadow records, writes retained draw blocks in place, and resolves built-in program keys once per demanded recipe. Prepared inputs carry skinning and custom-material membership. Draw sorting uses precomputed keys, light history no longer needs a separate per-object map in the optimized path, and shadow preparation borrows each proven enclosure once for all selected frusta. Ordinary meshes no longer copy an entire render item through particle preparation. There are no shader, light-capacity, draw-order or ordinary-mesh batching changes.

Ordinary native release, 800×600, the same RX 9070 XT hardware GLES device and saved 10,066-entity World, 60 frames per window:

| Render submission CPU | Control before | Final | Control repeated after | Reduction versus controls |
| --- | --: | --: | --: | --: |
| Moving, culled | 23.172 ms | 14.436 ms | 23.345 ms | 38% |
| Held, unculled | 47.519 ms | 22.294 ms | 47.282 ms | 53% |
| Held, culled | 20.561 ms | 13.687 ms | 20.294 ms | 33% |
| 256 mixed custom-material draws | 0.699 ms | 0.588 ms | — | 16% |

The mixed control ran after its final build. Table medians use the average of the two central CSV samples; the runner's console percentile selects one sample and can differ slightly. All executions are sequential, with compilation and other task-owned rendering stopped during timing. Shared-host variation still applies. Renderer time includes CPU preparation, GL submission and possible driver stalls; it is not isolated GPU execution time.

The final moving update-only median is **4.048 ms**. With rendering in the same frame, update is **4.791 ms**, render submission **14.436 ms**, completion wait **0.182 ms**, and measured total **19.560 ms**; medians of separate columns need not sum. This does not establish a 15 ms complete frame. The first held update-only window measured 2.246 ms before and 2.983 ms after (the repeated control is 2.200 ms); this pass does not establish a non-render improvement in every window. Moving update-only controls were approximately 4.35–4.65 ms. These update observations remain separate from the large, repeated rendering gains.

All six full-scene and four mixed-scene allocation windows report **zero warmed Rust allocation calls and requested bytes** over five counted frames each. Normal timing builds have no allocation instrumentation. Growing demand, structural changes, loading/recovery and graphics-driver allocations are outside that warmed Rust allocation claim. Shader lookup retains keys rather than program references; no World or GPU borrow survives a synchronous submission. Hidden draws retain selection history and selected-light validation errors, while skipping unused uniform packing. Authored bounds retain their conservative enclosure proof and particle bounds retain their separate behavior.

The final frame-pointer `perf` run uses the same 999 Hz CPU clock and 245-frame scope as the earlier profile. Its renderer contains 3,203 sampled CPU milliseconds, versus the prior 5,974. Dividing aggregate sampled CPU time by the 245 frames gives the following disjoint attribution; these values are sampling estimates, not individually timed stages:

| Renderer work | Prior sampled CPU/frame | Final sampled CPU/frame |
| --- | --: | --: |
| Light preparation, including its bounds | 9.977 ms | 5.258 ms |
| Additional culling/geometry | 3.984 ms | 1.410 ms |
| Per-draw built-in program access | 2.844 ms | 0.147 ms |
| Draw sorting | 0.863 ms | 0.515 ms |
| Shadow-slot assignment | 0.674 ms | 0.159 ms |

Light preparation remains approximately 40% of the smaller renderer CPU profile. It still evaluates contribution and hysteresis against the bounded candidate set; this pass preserves the scoring math. Additional frustum work remains approximately 11%. Memory-copy samples now return predominantly to light/shadow uniform-upload call sites; intervening driver frames may be absent from frame-pointer stacks, so these samples must not be presented as proof of whole-component Rust copying. Uniform submission, resource access, matrices and other device/renderer work remain in the unlisted remainder. Draw count has not been changed or established as the new limiting factor.

Validation: 42 focused core/renderer tests, including bit-exact cached-versus-scalar light ranking across 500 bounds/history cases; expanded and lean core/renderer Clippy; actual GLES custom-material and skinning/recovery scenarios; four actual WebGL tests using rebuilt expanded, shadow and particle configurations; all native frame/probe assertions. The skinning example now uses the existing Host-driven readiness helper so all resource lifecycle recipients run before presentation. No production lifecycle workaround was added. Every paired CSV frame has identical forward/shadow/triangle counts, and all nine paired full/mixed captures are byte-identical, including animated frames, texture transitions and stop restoration. Unculled draws remain 10,030 forward plus 40,120 shadow; held culled draws remain 8,562 plus 42.

Artifacts: `target/render-preparation/comparison.json`, `before`, `repeat-before`, `validated`, `mixed-before`, `mixed-after`, `allocations`, `mixed-allocations`, `final-sampled`, `final-renderer.data`, `final-sampling-summary.json`, `renderer-flamegraph.svg`, `frame-flamegraph.svg` and the isolated `pass.patch`. Executables and matching contracts/clients are retained in `validated-native-host`, `instrumented-host` and `validated-frame-pointer-host`. The earlier `after` and `final` directories are intermediate checkpoints. Existing [native commands](stress.md#run), [mixed-scene commands](stress.md#draw-and-spatial-comparisons) and [sampling commands](stress.md#draw-and-spatial-comparisons) reproduce the comparisons against preserved hosts.

## General draw submission follow-up

### Draw-count verification and sampled CPU profile

The follow-up attribution **does not support draw calls being the remaining limiting factor**. On the same Radeon RX 9070 XT hardware GLES device, a new ordinary release control without interception reproduces approximately 24 ms moving render submission. Two 60-frame controlled sweeps preserve the evaluated scene, light selection, resource access, culling, sorting and per-draw setup while changing only actual indexed submission. Each scene frame has normal controls before and after the rotating/reversed interventions.

| Moving render submission CPU             | First sweep | Repeated sweep |
| ---------------------------------------- | ----------: | -------------: |
| Normal control before interventions      |   26.523 ms |      24.617 ms |
| Skip every scene draw                    |   25.005 ms |      23.293 ms |
| Submit one quarter of scene draws        |   25.684 ms |      23.429 ms |
| Submit half of scene draws               |   25.272 ms |      23.357 ms |
| Keep draws, discard before rasterization |   25.537 ms |      22.822 ms |
| Skip draws and per-draw GL setup         |   22.115 ms |      20.126 ms |
| Normal control after interventions       |   25.177 ms |      22.812 ms |

The median frame attempts 8,603 indexed scene draws, including shadow draws. Interception independently verifies 0, 2,151, 4,302 or 8,603 actual submissions. A fullscreen presentation draw remains in the draw-only controls. The last control also skips presentation and the selected uniform, binding, attribute and buffer-update calls; framebuffer/pass setup and clears still execute. It is a diagnostic lower bound including renderer traversal and interception overhead, not a replacement rendering implementation.

Against each frame's mean of its bracketing normal controls, eliminating scene draws saves a median **3.1% / 2.8%** across the two runs. Skipping draws plus their GL setup leaves **85.2% / 83.6%** of submission time. The held culled case retains 83.6% in that last control; the held unculled case retains approximately 89%, despite suppressing all 50,150 indexed draws. The absence of a substantial quarter/half/zero-draw scaling trend rules out draw-count submission as the dominant CPU cost in these measured windows. Rasterizer discard also fails to produce a large reduction. Paired percentages are medians of per-frame ratios, not ratios of independently summarized table medians.

Linux `perf 7.2.5`, sampling `cpu-clock:u` at 999 Hz, supplies a separate CPU flame graph. The optimized profiling executable enables debug information and frame pointers. Its named, non-tail-called `profile_window` excludes loading and the preceding 200-frame animation warm-up; it includes 240 advancing measured frames and the measurement helper's five held settling frames. Stack filtering then selects only `RenderService::render`. Approximately 5,974 sampled CPU milliseconds fall within rendering out of 7,680 within the frame window. The following buckets are disjoint: light preparation takes precedence over its nested bounds/culling calls.

| Renderer CPU sample bucket                   | Share |
| -------------------------------------------- | ----: |
| Light preparation, including its bounds work | 40.9% |
| Additional visibility/culling                | 16.3% |
| Draw shader-program lookup                   | 11.7% |
| Draw sorting                                 |  3.5% |
| Shadow-slot assignment                       |  2.8% |
| Remaining renderer/device work               | 24.8% |

The graph exposes repeated engine work: light preparation ranks candidates and packs per-object light frames; visibility resolves component state and tests compound geometry against planes; shader lookup reconstructs a program source name and searches the source registry. Source identity comparison uses dynamic dispatch and byte-wise string comparison. Approximately 9.9% of renderer samples are in `memmove`, mainly under draw preparation; this overlaps the buckets above and does not prove a particular source-level copy is responsible. None of these findings requires automatic instancing to investigate.

The final frame-pointer trace reaches the intended window and renderer roots. The first DWARF attempt produced incomplete stacks and is excluded from the reported attribution. Flame graphs aggregate CPU samples, not chronological events, GPU execution time or blocked time. Filtering to the renderer thread excludes graphics-driver worker threads. Profiling and interception can perturb execution, so their timing values remain separate from the unwrapped ordinary-release control and the earlier optimization comparisons.

Artifacts are under `target/draw-attribution`: `sweep-60`, `repeat-sweep-60`, `unwrapped-control`, `sampled-fp`, `renderer-fp.data`, `stacks-fp.txt`, `renderer.folded`, `renderer-flamegraph.svg`, `frame-flamegraph.svg`, `timing-summary.json` and `sampling-summary.json`. Matching ordinary and frame-pointer executables, contracts and generated clients are retained in `native-host` and `frame-pointer-host`. The native saved World/assets and independent Blender fixture retain their earlier identities. See [reproduction and scope](stress.md#draw-and-spatial-comparisons).

Both sweeps pass exact normal-before/after image restoration, identical logical draw/triangle counts, exact held culling images, changed animated output, twelve independent Blender probes and World teardown. The unwrapped native run and sampled native window also pass. This pass adds only maintained profiling/example code and documentation; it makes no production renderer optimization, shader, batching or protocol change.

### General submission optimizations

The basic rendering pass reduces general submission overhead without changing draw order, geometry or batching. Ordinary native release on the same Ryzen 9 5900X / RX 9070 XT GLES device gives the following 60-frame medians. Render submission excludes World update and the final `glFinish` wait; it can still include driver stalls.

| Render submission CPU | Preserved control | Final | Change |
| --- | --: | --: | --: |
| Full scene, advancing with culling | 25.889 ms | 24.026 ms | 7% lower |
| Full scene, held without culling | 53.293 ms | 46.832 ms | 12% lower |
| Full scene, held with culling | 23.104 ms | 23.297 ms | No clear gain |
| 256 mixed custom-material draws | 1.354 ms | 0.694 ms | 49% lower |

Repeating the control after the final run gives 25.200 ms moving, 52.307 ms held without culling, and 1.258 ms mixed submission. Against that reverse-order control, the respective reductions are approximately 5%, 10% and 45%. The held culled case varies between runs and does not establish an improvement. These are shared-host measurements, not an isolated GPU execution benchmark. The final completion waits remain roughly 0.16–0.20 ms. The final advancing update-only median is 4.237 ms; this pass concentrates on rendering, and that small CPU-update difference is run variability.

The devices skip redundant program, vertex-array and blend changes, including resetting straight-alpha state after additive particles. Frame entry and resource deletion invalidate the tracked bindings; context recovery also refreshes cached device limits. Absent shader uniforms suppress unused base-texture and shadow setup. Generic vertex attributes keep their existing per-draw handling. Native parameter/instance buffers retain growing capacity and use buffer updates after growth, bringing native behavior into line with the browser path.

The new rendered mixed window exposed another generic cost: custom-material preparation rebuilt records and texture lists, cloned asset URI values, and uploaded material values during both preparation and submission. Retained records now own only copied draw flags, asset identities, packed words and texture names. Borrowed asset access and an iterator into the device avoid temporary texture vectors. Preparation validates limits and reserves parameter storage; each draw uploads its values once. The first device-only checkpoint still made 1,834 Rust allocations / 203,664 requested bytes per rendered mixed frame; the completed preparation path makes **zero**. This checkpoint is not the original executable's allocation baseline.

All six final full-scene profiling windows and all four final mixed windows, including actual mixed rendering, report **zero warmed Rust allocation calls and bytes**. Loading, structural changes, growing buffers, fallback diagnostics and graphics-driver internals are outside that claim. The final instrumented mixed submission median is approximately 0.712 ms with counters disabled during timing.

Every paired ordinary-release capture, draw/shadow count and triangle count matches exactly, including moving frames, culling, texture transitions and stop restoration. The held unculled case still submits 10,030 forward and 40,120 shadow draws; the held culled case submits 8,562 and 42. No cube-specific instancing, merging or shader changes are included. Remaining rendering costs include per-draw material/light uniforms, light selection, sorting, resource lookup and the actual driver/draw workload.

Validation passes 44 focused Rust tests, expanded and lean renderer Clippy, the actual GLES custom-material/lighting/shadow/recovery scenario, and four browser tests across custom materials, lighting and particles. The affected native fixture helpers now deliver Host lifecycle notifications before presentation; loader polling alone had left compiled resource selections stale in those older test arrangements. Material limits, fallback, alpha/additive behavior, device replacement and context recovery remain exercised. Formatting, TypeScript and repository checks accompany this pass; no full regression, commit, merge or push was requested.

Artifacts are under `target/draw-submission`: preserved `before-native-host` and `before-mixed-host`, `final-native-host` and `final-instrumented-host`, timing windows `before-release`, `final-release`, `repeat-before-release`, `before-mixed`, `final-mixed-release`, `repeat-before-mixed`, and allocation evidence `final-profile` / `final-mixed-profile`. `timing-summary.json` uses the conventional median of the two central samples; the example's console output chooses the upper central sample. Run/fixture identities and meaningful GLES captures accompany the results. See [reproduction commands](stress.md#draw-and-spatial-comparisons).

## Compiled geometry, render inputs and mixed animation

The follow-up implements all three optimization areas in `codex/stress-benchmark`: geometry preparation, render-input preparation, and mixed numeric/resource animation. The full advancing **native ordinary-release CPU update measures 4.683 ms median / 9.295 ms p95**, compared with the preserved prior executable's **8.497 / 13.505 ms**. That is a **45% reduction** in median update time. The Host still uses the RX 9070 XT GLES context for independent rendering checks; update-only windows make no renderer calls.

| Full scene, ordinary native release |    Before |    After |
| ----------------------------------- | --------: | -------: |
| Advancing update median             |  8.497 ms | 4.683 ms |
| Advancing update p95                | 13.505 ms | 9.295 ms |
| Held update median                  |  4.562 ms | 2.309 ms |

The workload remains 10,066 entities, 157 controllers and 40,092 drivers. Each timing window contains 60 frames. Earlier implementation checkpoints ranged from 4.1 to 4.4 ms moving median; the table reports the final ordinary executable, not the fastest checkpoint. The final instrumented executable, with counters disabled during timing, measures 4.125 ms moving median / 8.884 ms p95; its preserved instrumented control measures 8.459 / 13.253 ms. This is a shared host, so these windows do not establish a worst-case scheduling bound.

Separate five-frame instrumentation windows show the CPU work removed:

| Inclusive evaluation scope | Preserved control | Compiled preparation |
| -------------------------- | ----------------: | -------------------: |
| Geometry                   |    3.214 ms/frame |       1.354 ms/frame |
| Render inputs              |    2.895 ms/frame |       0.439 ms/frame |

Geometry retains rigid mesh bounds and precomputed local shape transforms. Root objects share one matrix/inverse cache with downstream consumers, using the analytic TRS inverse. The cache accompanies occupied Transform storage; the authored TRS and every animation key remain 40 bytes. Geometry's prepared state is retained behind its own allocation so cache growth does not enlarge every component/command enum. Skinned, morphing and joint-mapped geometry retain their specialized evaluation paths.

Render preparation retains entity membership, typed material access, mesh/texture selection, mesh-pose compatibility, debug geometry and light bindings. The frame loop reads changing values and finalized transforms through those bindings. Structural edits and resource availability changes rebuild selections; component release invalidates pointers synchronously. Live skin/palette validity remains an evaluated input. Resource releases preserve unrelated prepared lights and draw output.

Numeric drivers remain direct beside independent resource drivers, including properties of the same material. Unit-weight dynamic numeric drivers retain their descriptor/offset and use the current component-owned buffer base after growth. Exclusive discrete drivers keep the applied step between key changes; producer commands, external writes, owner changes, failure and source suspension invalidate that retained contribution. Numeric updates therefore do not clone material descriptors, strings or resource ownership. Specialized skeletal source writes preserve declaration-ordered rebasing, and combined/weighted numeric operators retain their output guards.

### Mixed-property comparison

A new [maintained native fixture](stress.md#evidence-and-limits) contains 256 cubes, 13 controllers and 2,560 drivers: eight numeric material properties, one texture switch and one independent scalar per cube, plus 64 extra material properties per cube. It is authored over the real generated WebSocket client, saved with the target contract, and loaded by the GLES profiling Host.

| Advancing native update | Generic-property control | Compiled property bindings |
| --- | --: | --: |
| Median | 8.712 ms | 0.188 ms |
| p95 | 12.289 ms | 0.193 ms |
| Warmed Rust allocations/frame | 118,023 | 0 |
| Requested allocation bytes/frame | 8,274,092 | 0 |

This is approximately **46× faster**. Both variants use the same executable and the new geometry/render preparation; the toggle isolates property binding and discrete retention, rather than reconstructing every historical subsystem. Initial, switched and stopped/restored GLES captures are byte-identical between variants. Transition/loading/structural work is outside these warmed windows and may allocate; resource transitions still perform ownership processing and can copy a component. Extra compiled metadata and work buffers are retained at preparation boundaries rather than rebuilt every frame.

All six final full-scene profiling windows and all three optimized mixed-property windows report **zero Rust allocation calls and bytes**. The full and mixed warmed windows also enter no generic component commits. These counters cover the Rust application allocator, not allocations inside the graphics driver. The full scene's held capture retains SHA-256 `a973454dd53bae25312470980a23c09e4d33b6a2bbe2ef8011bf2dbc863e8333`; independent Blender probes, animation and exact culling pass.

Validation: 202 focused Rust tests across 19 binaries, a follow-up animation/hierarchy run after the final traversal refinement, expanded and lean Clippy, and ten Miri tests covering storage/companions, mixed properties, hierarchy, geometry reuse and asset-release bindings. The expanded custom-material browser scenario passes mixed animation, backward/forward seeks, stop restoration, skin/pose/shadow comparisons, persistence and context recovery. The rebuilt Blender browser stress scenario passes in 35.851 seconds, including warmed allocations, exact state/pixels, culling, Rigify and particles. Formatting, TypeScript, repository and workspace checks pass. No full regression, commit, merge or push was requested.

Final native artifacts: `target/evaluation-bindings/complete-{release,profile}`, preserved controls `before-{release,profile}`, and `mixed-complete-{control,bound}`. Run identities include matching contracts, executable/source hashes and fixture identity. `mixed-bundle` contains the reusable saved mixed World and assets; `followup.patch` isolates this change from the verified previous experiment snapshot. Browser evidence: `target/integration-artifacts/custom-materials/ipp-browser-custom-materials-cNUVpX` and `target/integration-artifacts/stress/ipp-browser-blender-stress-benchmark-fWMfG6`.

## Compiled numeric publication and retained queries

The complete advancing native CPU update is now below the user's 15 ms target on the full 10,066-entity, 40,092-driver scene. Ordinary release measures **8.412 ms median / 13.249 ms p95**, excluding rendering, compared with **32.715 ms median** before direct target publication. Held update is 4.527 ms, previously 26.347 ms. These are 60-frame windows on the shared Ryzen 9 5900X; they are measured windows, not a worst-case scheduling guarantee. Actual AMD Radeon RX 9070 XT GLES remains enabled for separate capture/culling checks; the update-only windows make no renderer calls.

A controlled comparison immediately before the final numeric-patch correction uses one instrumented executable, with counters disabled during timing and five additional allocation/profiling frames per window:

| Implementation | Held update median | Moving update median | Moving p95 |
| --- | --: | --: | --: |
| Mode 14: compiled curves, general component commits | 28.076 ms | 32.755 ms | 40.808 ms |
| Mode 16: cached numeric destinations/publication | 4.554 ms | 8.560 ms | 13.681 ms |
| Mode 17: also cached hierarchy access | 4.504 ms | 8.314 ms | 13.624 ms |

After removing the final two particle-clock patch commits and extending independent numeric binding to the remaining constraint, geometry, mesh-pose and particle float fields, the final instrumented moving window measures 8.451 ms median / 13.307 ms p95. The preceding patch-only checkpoint measured 8.095 ms ordinary release and 8.174 ms instrumented; the repeat difference is within earlier run variability and is not an isolated estimate of the added field support cost. All final instrumented windows report **zero Rust allocation calls and requested bytes**. The final full scene records **zero generic component commits** during the measured frames, versus 20,052 per frame in the control. There are still 84 staged numeric publications per frame for operators requiring combined output guards; these use cached typed destinations, take about 0.003 ms total in the profiled window, and do not invoke generic preparation, System validation hooks or lifecycle reconciliation.

`ComponentBinding<T>` centralizes typed cell access with no per-element virtual dispatch or allocation. `ComponentQuery<T>` retains sorted component membership and updates it at lifecycle boundaries. Pointers originate from `UnsafeCell<MaybeUninit<T>>` in fixed boxed pages, never a temporary mutable component reference. Numeric access borrows the owning World's storage; lifecycle owners discard bindings before removal/replacement/reuse. Shared helper adoption covers animation, hierarchy, scalar constraints, skeletons, skinning, geometry and particles. Scalar constraints retain order/restoration capacity; asset demand uses change revisions and dirty flags; particles no longer compute transforms for the 10,000 entities without particle components.

Numeric notifications invalidate dependent evaluated outputs once per controller batch. External System observers must implement `before_numeric_update` to observe compiled numeric animation; structural callbacks retain their mutation/release role. Combined arithmetic/range guards for weighted or coupled values are distinct from structural/type validation. Immutable curve backing remains shared with an inline active segment; whole-curve copies are still disabled by default for the memory/timing reasons in the earlier locality comparison below.

Hierarchy's initial/final propagation scopes together fall from approximately 0.067 ms in mode 16 to 0.022 ms in mode 17. This scene contains relatively few parented objects, so that optimization explains only a small part of the complete update difference. The large gain is animation publication. Remaining inclusive profile scopes are animation evaluation 3.43 ms, restoration 0.43 ms, geometry 3.18 ms and render-input preparation 2.77 ms. Geometry and render preparation still repeat some source/model/selection work; those are the next substantial CPU opportunities. CustomMaterial numeric patches and particle clocks publish through bound component access too. The final sweep also covers constraint scale/bias, bounding/picking geometry display fields, mesh-pose weight and particle emitter/sprite float settings, including weighted patches and binding-time key/handle proofs. Dynamic properties still resolve their stable identity against their current byte buffer, and some other component operators and resource-bearing animation retain checked/structural paths. This is not a claim that every possible scene is fully compiled or allocation-free.

Final artifacts are under `target/cpu-update-15ms/numeric-sweep-{profile,release,native-smoke}`. The preceding patch checkpoint is under `final-patch-{profile,release,native-smoke}`, and the controlled preceding comparison is under `final-bound-{control,hierarchy-control,profile,release}` in the same directory. Each contains its executable/environment identity, per-frame timings, independent Blender probes and meaningful frame captures. All held captures remain byte-identical to the preceding native baseline. The earlier first direct-binding run and intermediate query run are retained separately and are not mixed into the final table.

Focused validation passes 197 Rust tests across 17 binaries, expanded core/server Clippy, formatting, TypeScript, repository and workspace checks. Miri passes storage growth/replacement/drop tests, numeric observer/lifetime tests including a weighted output, quaternion writes, deferred hierarchy removal dynamic-buffer relocation/property loss, and resource-owning numeric lanes including weighted output guards and stable particle buffers; nightly emits only its newer deprecation warning for the repository-pinned compiler's existing atomic API. Three unrelated scaling tests remain excluded. No full regression, commit, merge or push was requested.

The final rebuilt browser stress scenario passes all WASM/worker/generated-client, transform, Rigify, particle, culling, memory-growth and warmed allocation assertions (34.472 seconds; process total 34.916 seconds). Evidence: `target/integration-artifacts/stress/ipp-browser-blender-stress-benchmark-l8jQKZ`. A fresh native WebSocket import with the matching generated client also passes its small-scene probes/culling/teardown checks. Full held captures retain SHA256 `a973454dd53bae25312470980a23c09e4d33b6a2bbe2ef8011bf2dbc863e8333`.

## Compiled binding and cache locality follow-up

The current target is **15 ms for the complete non-render update**; GL optimization is outside this follow-up. Preparation now resolves typed track access once, retains immutable tracks under the asset release barrier, and stores the controller duration. Active drivers trust synchronous target invalidation. Payload unload releases their track access and freezes the clock until preparation succeeds again; changing a skeleton's pose input or a particle cache source also marks the relevant controller for preparation. Producer/overlay commits maintain public restoration values instead of refreshing every bound property every frame. Fixed field types are revalidated when their lifetime changes, not on each numeric commit.

The full-scene locality comparison uses one native release executable with instrumentation compiled in but disabled during timing, 60 frames per window, the same scene and 40,092 drivers. Sampling-only rows include 40,088 scalar/quaternion tracks at a time between baked keys; the four joint-pose tracks and component staging/commits are excluded from that microbenchmark. Both driver sampling blocks are shown. Whole updates include all drivers and advancing particles, with no rendering calls in `moving-update-only`.

| Track backing | Numeric sampling only | Held update only | Moving update only | Copied track payload | Track preparation |
| --- | --: | --: | --: | --: | --: |
| Shared typed tracks, cached key interval (12) | 3.58 / 3.45 ms | 36.19 ms | 41.08 ms | 0 | 4.47 ms |
| Private copies of complete tracks (13) | 2.85 / 2.89 ms | 33.18 ms | 42.03 ms | 544.99 MB | 68.76 ms |
| Shared tracks, inline current segment (14) | 1.39 / 1.40 ms | 28.44 ms | 35.53 ms | 0 | 5.26 ms |

Copying whole tracks helps the held sampling microbenchmark by roughly 17–20%, but does not establish a moving-update gain. An earlier opposite-order sequence measured moving updates at 41.07 ms shared, 39.66 ms copied and 41.28 ms shared again. The repeated result places the full-copy effect within run-to-run variation, while duplicating approximately 520 MiB of immutable track payload. Copies use private allocations in binding order, not a globally packed key arena.

Caching the current interpolation segment is the selected implementation: about **60% faster held numeric sampling and 14% faster complete moving updates** than shared tracks with only an interval cursor. The active scalar/quaternion segment data occupies **4,248,488 bytes** inside driver allocations. Experimental driver structs reserve that capacity in every variant; populating it makes no new heap allocation. Segment changes refresh only the needed key/value data, reuse the adjacent interval for ordinary playback, and fall back to binary search on arbitrary seeks. Complete keys remain shared. Preparation timing re-resolves track backing for existing target bindings; it excludes target/controller construction, source decoding, and the first segment fill. Copied payload bytes exclude allocator/Arc bookkeeping.

The final **ordinary native release**, with profiling code compiled out, measures **26.35 ms held and 32.72 ms advancing update-only**, versus the previous 84.56 ms held update at the same scene time. The new moving window is explicitly free of renderer calls; the older 91.34 ms figure was the update portion of frames that also rendered. The 15 ms complete-update target is not yet reached. GL implementation was unchanged in this follow-up.

The remaining largest cost is component publication: 20,052 commits per frame (restoration plus sampling), about 20.45 ms in the inclusive instrumented moving scope. Each goes through value preparation and target lookup, all System validation/before/after callbacks, capacity/identity checks, component storage writes and overlay bookkeeping. The nested measured validation/before/storage/after scopes are 3.31/5.30/3.15/2.39 ms; setup/cleanup and instrumentation account for additional enclosing time. Numeric sampling plus staging is 9.10 ms, including current-value reads and schema writes. These are inclusive profiling measurements, not additive to their parent AnimationSystem rows or identical to ordinary release cost. A narrow numeric publication path and consolidated affected-System notifications are the next optimization; callback semantics and ordered resource/discrete changes still need to be preserved.

All six windows in every full native variant have zero warmed Rust allocation calls and requested bytes. Every run passes 12 independent Blender probes, exact culling, meaningful capture and teardown checks. Held captures remain byte-identical to the previous native baseline (`a973454dd53bae25312470980a23c09e4d33b6a2bbe2ef8011bf2dbc863e8333`). Runs and identities are under `target/cpu-update-15ms/{final-shared,final-copies,final-segments,final-release}`; the first comparison and shared repeats are retained alongside them. The same checks pass after a fresh native generated-client/WebSocket import of the small scene, with 0.297 ms moving update-only.

At that earlier checkpoint, focused validation passed 143 Rust tests across 12 binaries, including the new pose-source suspension regression. Coverage includes interpolation/seek equivalence, quaternion normalization, old-track release before unload, restoration/overlays, component replacement, partial failures and shared asset lifecycle. Expanded Clippy and the instrumented WASM target check pass. Three unchanged scaling tests were excluded from the focused package pass; no full regression, commit, merge or push was requested.

The first browser attempt caught one 4 KiB shadow-caster buffer growth after particle warm-up; all animation/core categories stayed at zero. The benchmark now traverses the complete camera/light timeline once, then restarts and warms particles before its moving allocation window, so warmed coverage includes changing visible caster sets. No GL source change was needed. That earlier mode-14 browser check passed all assertions in 29.207 seconds, along with its source checks.

## Ordinary native release

These windows use native `--release` optimization without profiling instrumentation, an 800 × 600 EGL pbuffer, completed GPU work on every measured frame, and the full 10,066-entity / 157-controller / 40,092-driver scene. Assets come from a saved native World imported by the real generated WebSocket client and standard Blender adapter. The baseline executable/schema/client are preserved under `target/render-buffer-reuse/native-before-host/`.

| Held workload | Before: mode 8 | Driver index: mode 9 | Index + skin invalidation: mode 10 |
| --- | --: | --: | --: |
| Update only | 715.05 ms | 623.23 ms | 84.56 ms |
| Unculled complete frame | 766.42 ms | 684.01 ms | 138.92 ms |
| Culled update | 725.43 ms | 618.61 ms | 89.70 ms |
| Culled render submission | 24.17 ms | 23.38 ms | 22.96 ms |
| Culled remaining completion wait | 0.21 ms | 0.21 ms | 0.20 ms |
| Culled complete frame | 749.93 ms | 644.77 ms | 113.16 ms |
| Culled p95 complete frame | 817.85 ms | 758.08 ms | 127.85 ms |

Rows are individual medians and therefore need not sum. All table columns have 30 frames per window. This is approximately an 8.5× reduction in update-only time and a 6.6× reduction in held culled frame time. An earlier 60-frame optimized run measured 86.77 ms update-only and 117.82 ms held culled, consistent with this repeat. Shared-machine load causes substantial variation: IDE indexing and unrelated test processes were active. Treat these as diagnostic measurements, not a tight regression threshold or a hardware throughput limit.

Culling changes main/shadow draws from 10,030 / 40,120 to 8,562 / 42. Native held captures match exactly before/after optimization and culling (SHA256 `a973454dd53bae25312470980a23c09e4d33b6a2bbe2ef8011bf2dbc863e8333`). Twelve independent Blender position/rotation probes at 0, 2.5, 5 and 10 seconds pass.

The repeated 30-frame normal-release moving window measures **116.07 ms total**, 91.34 ms update, 24.50 ms submission and 0.22 ms remaining completion wait (p95 total 123.58 ms). The initial 60-frame moving mode-10 window records 203.30 ms total with 154.19 ms update and 48.59 ms submission; its p95 is 305.20 ms. A separate instrumented-build timing window, with counters disabled, records 141.25 ms total / 109.90 ms update / 32.09 ms submission and p95 198.09 ms. These additional windows use different lengths and machine load; they establish continued moving-scene cost rather than a controlled speed ratio. Baseline moving timing was 743.55 ms over 30 frames.

`glFinish` measures only work still pending after submission. Driver work and synchronization can happen inside rendering calls, so its approximately 0.2 ms wait must not be reported as total GPU time. This benchmark does not yet include GPU timer queries or accelerated browser measurements. Native uses the ordinary release profile while the historical WASM benchmark uses `release-small`; the difference is not solely WASM versus native or hardware versus software.

## CPU pathology and allocation evidence

The pre-fix native instrumented attribution gives 369.24 ms/frame to animation evaluation and 331.32 ms to restoration; all other individual System stages are below 3 ms. Allocation counting is zero in those frames. Allocation elimination had not eliminated redundant work.

After the fixes, the corresponding instrumented stages are 64.91 ms evaluation, 21.70 ms restoration, 3.22 ms render preparation and 1.49 ms particles. Instrumentation itself adds overhead, so primary frame numbers remain the separate ordinary-release runs.

Two fixes preserve the normal lifecycle path:

- Each controller now indexes its bound driver ordinals by target entity/component. Validation and invalidation visit only bindings affected by the commit. Binding/pruning rebuilds the index; stopped/replaced bindings cannot retarget reused storage.
- Skinning previously scanned every entity on every component commit to clear palette validity. Roughly 20,000 restoration/sampling commits multiplied a 10,066-entity scan, even though only four humans have skins. Once the first scan invalidates the palettes, further scans are redundant until skinning evaluates again. The existing refresh flag now records that state. Newly inserted/replaced skins start invalid and runtime preservation transfers the already-invalid palette. Synchronous callbacks and invalidation before reuse remain active.

The palette test exercises repeated ordinary numeric commits between actual skinning evaluations, checks both affected and neighboring palettes are unavailable immediately, and verifies recovery with stable palette storage. Existing deletion, replacement, failed pose and recovery tests also pass. No callback-bypassing unsafe write path was adopted.

The final instrumented native full run makes **zero Rust allocation/reallocation calls and requests zero bytes** in all five-frame held, culled, advancing-particle and paused-moving-pose windows. These counters cover Rust application allocations, including rendering; they exclude graphics-driver allocations. Mode 8 had already removed warmed renderer scratch allocations; modes 9/10 remove CPU traversal work. The browser mode-8 full and small allocation/capture evidence is in the [renderer results](stress-results.md#retained-renderer-buffers-mode-8).

## Remaining costs and evidence

The small 64-cube / 130-entity native fixture measures 0.46 ms update-only, 1.19 ms held culled and 1.34 ms moving culled over 60 frames; its independent Blender probes and culling/animation captures pass. The full scene remains above a 60 Hz budget. Animation still samples and restores 40,092 drivers through per-component commits, multiple observers and dynamic dispatch. Drawing still submits thousands of individual cubes. Those are separate remaining opportunities; preallocation cannot remove the computation or submission count. Custom-material/error/resource-growth paths and JavaScript allocations are not covered by this scene's zero-allocation claim. Clip loading and retained WASM capacity remain separate memory concerns.

Native evidence is under `target/render-buffer-reuse/`: `native-full.log`, `native-binding-index/`, `native-skin-invalidation/`, `native-final-release/`, `native-final-smoke/`, `native-stage-profile/` and `native-final-profile/`, with their corresponding logs. The original baseline CSV/captures are under `target/stress-benchmark/native-full/`; native imported assets/World are in its `bundle/`. The final profile includes per-frame timing CSVs, stage CSVs, exact allocation totals, captures and build/run identity. The instrumented full run verifies independent Blender probes, rendered animation, exact culling and clean teardown.

## Is an animation update below 15 ms realistic?

It is a reasonable engineering target for this fixture, but the complete update has not reached it. A read-only prototype resolves the actual bound scalar/quaternion tracks once, retains phase-local Rust references, and samples them in typed loops. At 0.5 seconds plus 1/120 second, between the 24 Hz baked keys, **40,088 tracks take 9.86 and 9.73 ms**. The existing driver API takes **21.17 and 24.02 ms** for the same samples. Blocks run in driver/resolved/resolved/driver order, with five warmups and thirty measured passes each. Every resolved sample is compared exactly with the ordinary driver result before timing; counts also match. This uses safe references, with no World mutation or pointer escaping the borrow.

The prototype includes real key lookup/interpolation over the scene's resident curve data; it excludes component staging, commits and four joint-pose drivers. Binding resolution and allocation occur before timing. It groups scalar and quaternion tracks into separate typed loops, so the improvement includes traversal/layout effects as well as removal of repeated asset lookup, type resolution and driver dispatch. It is an experimental cost floor, not an integrated 10 ms animation updater or evidence that spelling a write with `unsafe` makes it faster.

Nested timing scopes identify the rest of the work:

| Scope | Native instrumented cost per frame | Work |
| --- | --: | --- |
| Sampling and temporary component staging | 33.6–34.6 ms | Curve access/math, current-value reads and assembling component updates |
| Applying component updates | 36.8–42.8 ms | 20,052 commits across restoration and evaluation |
| Collecting restoration updates | 4.7–5.7 ms | Reading target identities and assembling original values |
| Commit validation, included in applying above | 14.2–15.7 ms | Per-commit observer dispatch and target/value validation |
| Before-commit work, included in applying above | 11.5–14.0 ms | Per-commit observer dispatch and binding/output invalidation |
| Storage writes, included in applying above | 3.2–3.9 ms | Applying prepared values and component bookkeeping |
| After-commit work, included in applying above | 2.4–3.2 ms | Post-commit dispatch and cleanup |

These are inclusive instrumented scopes; do not sum parent and child rows. More timing scopes add overhead. The normal-release update remains approximately 85–91 ms. Additional animation work includes refreshing originals, repeated binding/readiness checks and recomputing controller clip durations. Particles take about 1.4 ms and core render preparation about 3 ms; neither explains the animation cost.

The next work should target resolved typed curve access (including cached key intervals), retaining binding/readiness/duration information until it changes, and a numeric update path that amortizes staging and notifications while preserving lifecycle invalidation and ordered behavior. Replacing scalar stores with unsafe pointer syntax alone leaves these costs intact. The later user clarification scopes the 15 ms target to non-render evaluation; rendering/submission is excluded from the current optimization goal.

Evidence: `target/render-buffer-reuse/native-detailed-profile/` and `native-resolved-profile/`, including `curve-sampling-only.csv`, nested stage CSVs, zero-allocation totals, captures and exact build identity. Both full held runs pass Blender probes, exact culling and teardown. The latest ordinary-release measurements and moving-allocation evidence remain valid: the additional code is opt-in instrumentation and read-only comparison support.

## Validation and recovery

The combined core/GL run passes 330 tests across 36 binaries (three unchanged scaling cases excluded). Expanded Clippy and the instrumented WASM target check pass. Maintained lighting, particle, custom-material and skinning browser suites pass six rendered scenarios; the debug-geometry file passes two more. The final browser stress run (`...-yNEfTH`) compares modes 10/8/10 with exact state/pixels, culling, memory growth, isolated particles/Rigify and zero held/moving Rust allocations. Fresh native generated-client/WebSocket import and instrumented smoke profiling pass after the detailed-probe addition. Repository/workspace, TypeScript, regression-catalog and pinned formatting checks pass. This is focused validation; no full regression, commit, merge or push was requested.

Source, native bundles/builds and follow-up evidence are preserved in `/home/dev/ipp-worktrees/performance-evidence/stress-benchmark-20260914/render-native-followup.tar.gz`, with a manifest and verification file beside it. The earlier `scene-source-and-evidence.tar.gz` in that directory retains both full Blender source/bake files and the original browser bundles. Source remains uncommitted in `/home/dev/ipp-worktrees/stress-benchmark` on `codex/stress-benchmark`, based on `f524deb`; the primary checkout's concurrent work is preserved.
