# Per-frame allocation sweep

Historical measurements from the experiment worktree. For current maintained commands and opt-in scope, see the [benchmark guide](stress.md).

The [joint/material/response follow-up](reuse.md) supersedes the corresponding residual findings below. This document preserves the first sweep's mode 0 → 3 measurements.

This continues the [initial animation/pointer experiment](README.md) in `/home/dev/ipp-worktrees/performance-experiments`, branch `codex/performance-experiments`, Beads `ipp-yto`. It uses the same inherited source snapshot, not later concurrent edits in the primary checkout. Changes remain experimental and uncommitted.

The target is zero allocations after capacity warmup for an unchanged workload. The scalar update and scalar decoder now enforce that target with allocator assertions. The robot still allocates; the result tables below quantify the remaining gap. Startup, a new workload peak, structural edits, new resource content, owned messages and error reporting are separate cases.

## What changed

Mode 0 disables the allocation optimizations; mode 3 enables them **while retaining ordinary component validation, commit callbacks and lifecycle invalidation**. Modes 1/2 remain the earlier callback-bypassing direct-write prototypes and are not used for the new allocation comparison. Ordinary builds of this experimental branch enable the safe allocation changes; the profiling feature provides the A/B switch. No changes have been adopted in the primary checkout.

| Area | Why it allocated | Implemented change |
| --- | --- | --- |
| Animation field access | Enumerating every component field constructed a vector and cloned unrelated strings, byte arrays and dynamic metadata for one numeric property. This repeated inside global binding checks. | Generated exact-field getters; borrowed staged/typed access; stack storage for numeric dynamic values and quaternion lanes. |
| Commit validation/invalidation | Every evaluated write checked unrelated bindings and cloned their components. Single-component mutation maps allocated tree nodes. | Filter by affected entity/component before reading a binding; keep the common single entry inline, spill to ordered storage for larger changes. All selected callbacks still run. |
| Controller sampling/restoration | Temporary component maps, controller ID lists, readiness sets, and controller removal/reinsertion rebuilt storage. | Reusable sorted staging vectors and identity/readiness buffers; detach controller values while retaining map nodes, reinstall before callbacks. Explicit controller deletion still removes the node. |
| Hierarchy/aiming | Per-commit descendant vectors and visited sets, and a temporary changed-entity collection. | Reusable epoch-marked traversal storage and direct iteration over changed keys. |
| Assets | Owned temporary URI keys, duplicated demand unions, repeated observed-source reconstruction, and an allocated polling key set. | Borrowed logarithmic source lookup, merge ordered demand iterators, compare retained demand, dirty observation membership, traverse existing catalog slots. Resource transitions still use lifecycle barriers. |
| Skeleton/skinning | Rebuilt entity lists, local poses and temporary palettes despite unchanged sizes. | Reuse system scratch capacity; fill and validate before copying to the existing stable component buffers. Size/source changes use the existing replacement path. |
| Render preparation | Fresh output vectors and built-in program recipe/name collections. Custom numeric uploads rebuilt GLSL declaration strings and packing temporaries. | Refill output vectors, reuse recipe/key buffers, borrow program names, pack numeric words into retained capacity. |
| Host/queues | Fresh Host frame identity collections and replies. Host connections copied message bytes after a validation decode, then decoded them again at admission. | Retained frame/reply capacity, idle connection fast path, decode once into an owned request, recheck the session fence at admission. |
| Batch command buffers | The outer queue retained its capacity, but each consumed batch dropped its separate operation vector. | Caller-owned decoder buffer; World recycles consumed command vectors after clearing all payloads. Initial capacity is 256 operations and grows for larger batches. Pending queue reservation is 64 requests, within existing backpressure policy. |
| JS/WebGL | New typed-array views for every uniform call; GPU parameter/instance buffers re-specified every draw. The initial profiler also allocated its frame records. | Cached whole-memory views with checked ranges and refresh after memory growth; WebGL offset/length overloads; grow GPU capacity only when needed and upload with bufferSubData. Profiler uses a preallocated typed array. |

A `Vec`/`VecDeque` can keep capacity after `clear` or draining. A `BTreeMap`/`BTreeSet` frees its nodes when cleared: calling `clear` is insufficient there. Buffers moving through a queue also need an explicit ownership return path. Preallocating only the outer queue does not retain the vectors or strings inside its messages.

The command pool contains empty vectors, not old commands or session identities. At 176 bytes per native `Command`, 256 slots retain 44 KiB per buffer in this build; concurrent batches need independent buffers. Capacity follows the largest observed concurrent workload until World teardown. Dynamic message payloads, correlated outcomes and encoded responses can still allocate. Existing protocol size and queue admission limits remain in force.

## Measurement method

Native optimized `release` executable with the default capability selection, pinned to CPU 22 on the same shared Ryzen 9 5900X workstation. Browser optimized `release-small`, Chromium 153 with SwiftShader/WebGL 2. Timings run with counters disabled; separate windows measure exact Rust/WASM allocation and reallocation request counts and requested bytes. Exclusive nested categories sum exactly to the global totals. The allocation scope implementation is intended for one measured Host thread, not concurrent independent Worlds on multiple threads.

Requested bytes are allocation traffic, not retained memory, peak live heap, a leak, GPU allocations or JavaScript allocations. Rust and JS results are separate. The changed modes include small common refactorings of scratch construction, so mode 0 is the current instrumented control path; it is not a byte-for-byte historical binary.

The native fixture uses real controllers, decoded uploaded clips and normal World preparation/poll/evaluation. Its decoder measurement uses actual wire requests but excludes transport/reply processing. Browser measurements use the real gallery, generated client, worker transport, WASM, assets and GL presentation. The robot has 69 entities, 8 controllers, 41 declared drivers including joint tracks, 43 draws and 95,101 triangles at 718 × 539 pixels. Held-pose comparisons require exact entity/controller state and zero differing captured pixels. Moving comparisons verify advancing playback and changed rendered pixels. Each mode-3 block additionally forces WASM memory growth before capture to check cached-view refresh.

## Native before/after results

Medians of two opposite-order timing blocks (0, 3, 3, 0); 100–5000 warmed updates per block. Allocation counts come from separate 30-frame windows. These rows include World prepare, asset polling and evaluation, with no renderer or HostSession transport.

| Workload | Mode 0 | Mode 3 | Allocations/update, 0 → 3 |
| --- | --: | --: | --: |
| 1 idle scalars | 0.0014 ms | 0.0009 ms | 6.0 → **0** |
| 1 scalars / 1 controller | 0.0049 ms | 0.0026 ms | 55.0 → **0** |
| 64 idle scalars | 0.0023 ms | 0.0018 ms | 6.0 → **0** |
| 64 scalars / 1 controller | 1.2281 ms | 0.1624 ms | 17,398.0 → **0** |
| 64 scalars / 64 controllers | 1.2712 ms | 0.2562 ms | 17,524.6 → **0** |
| 512 idle scalars | 0.0085 ms | 0.0079 ms | 6.0 → **0** |
| 512 scalars / 1 controller | 81.7590 ms | 5.2633 ms | 1,056,458.0 → **0** |
| 512 scalars / 512 controllers | 93.3061 ms | 11.4353 ms | 1,057,408.2 → **0** |

For 512 drivers in one controller, this is 81.76 → 5.26 ms (15.5× faster) while preserving callbacks. Requested allocation traffic drops from 194,261,874 bytes/update to zero. With 512 controllers, 93.31 → 11.44 ms (8.2× faster). This is substantially slower than the earlier callback-bypassing prototype; those variants implement different observer semantics and must not be conflated.

Scalar decoding of 512 operations (10,781 wire bytes): **16.40 → 16.44 µs**, **1 → 0 allocations**, **90,112 → 0 requested bytes**. The decoder gain is removal of allocation traffic, not a measured throughput improvement. One- and 64-operation decoder fixtures also assert zero warmed allocations with reuse. The codec test exercises 1/256/1,024/3/0/1,024-operation bursts, preserved buffer addresses within capacity, growth, truncation recovery and session rejection.

Evidence: `target/allocation-sweep/native-final.log`, `native-final-error.log` (empty), `native-summary.json`.

## Robot before/after results

| Metric | Held mode 0 | Held mode 3 | Walking mode 0 | Walking mode 3 |
| --- | --: | --: | --: | --: |
| Timed frames | 203 | 204 | 103 | 103 |
| Rust allocations/frame | 28,028.0 | 200.0 | 28,096.8 | 298.1 |
| Rust requested bytes/frame | 5,406,170 | 50,014 | 5,414,345 | 63,895 |
| Mean Host tick, ms | 26.22 | 23.44 | 23.11 | 21.20 |
| Median Host tick, ms | 23.90 | 21.95 | 22.10 | 20.80 |
| p95 Host tick, ms | 40.40 | 32.10 | 32.20 | 25.80 |

Held pose: **28,028 → 200 allocations/frame (99.29% fewer)** and **5,406,170 → 50,014 requested bytes/frame (99.07% less)**. All four mode blocks match entity/controller state and captured pixels exactly; each optimized block survives forced memory growth. Walking: **28,096.8 → 298.1 calls/frame (98.94% fewer)**; playback advances and completed captures change as expected. Walking can also trigger particles and resource activity, so its allocation count is not a constant warmed held-pose count.

Observed mean Host tick improvements are 10.6% held and 8.3% walking; median improvements are 8.2% and 5.9%. These are modest scene-level gains relative to the allocation reduction. They include software graphics submission/waits and exclude the Host timer gap between ticks, so they are not FPS or hardware-GPU speedup claims. Other workspace jobs were active on this shared machine. Held timing block means were 30.13 / 25.62 / 21.27 / 22.35 ms in mode order 0 / 3 / 3 / 0, showing substantial drift; walking means were 23.87 / 21.70 / 20.70 / 22.36 ms. Do not treat these percentages as precise universal gains.

The exact held mode-3 allocation categories are:

| Exclusive category     | Calls/frame | Requested bytes/frame |
| ---------------------- | ----------: | --------------------: |
| ipp.animation.restore  |          63 |                 6,501 |
| ipp.animation.evaluate |          40 |                 4,169 |
| ipp.render.evaluate    |           3 |                   276 |
| animation.originals    |           6 |                 2,584 |
| animation.sample       |          37 |                 6,232 |
| animation.demand       |          12 |                 1,415 |
| host.tick              |           4 |                   120 |
| gl.render              |           9 |                14,527 |
| gl.draw                |          26 |                14,190 |

JavaScript heap sampling, filtered to the production worker `frame` call tree, estimates **60,848 → 25,550 allocated bytes/frame (58.0% less)**. This uses 64 baseline and 63 optimized sampled frames, separate from timing and Rust profiling. Two blocks agree on direction: baseline 61,488 / 60,208 bytes/frame versus optimized 26,089 / 25,029. Remaining sampled activity is concentrated in draw/import callbacks (`drawMesh`, `set_instances`, lighting, shadow, skin and alpha state). V8 sampling is statistical; these are not exact allocation counts. Playwright serialization, diagnostic readback and other non-frame-rooted allocations are excluded from this comparison.

Evidence:

- Held + JS sampling: `/home/dev/ipp-worktrees/performance-experiments/target/integration-artifacts/gallery-combined/ipp-browser-robot-performance-oqJN7F/profile.json`.
- Walking: `/home/dev/ipp-worktrees/performance-experiments/target/integration-artifacts/gallery-combined/ipp-browser-robot-performance-oSMCc8/profile.json`.
- Combined reduction/call-category summary: `target/allocation-sweep/robot-summary.json`.
- Matching completed captures, browser environment records and cleanup evidence accompany each profile.

## Remaining allocation sites and costs

The measured robot remainder is not allocator noise. Main paths still own temporary values:

- Animation clones complete affected CustomMaterial values and sparse restoration values. Dynamic descriptor strings/maps and selected owned properties remain allocated, even though unrelated-field enumeration is gone.
- Joint sampling still returns owned pose vectors for current values, originals, interpolation, additive/reference sampling and output. The per-frame sampled-joint map/set also allocates. Reusing the Skeleton preparation/palette buffers does not remove those sampler vectors.
- Resource-bearing animation targets still rebuild evaluated source demand. Only targets provably free of resource-valued/dynamic fields retain the clip-demand set in place. Resource changes must remain observable.
- RenderService still receives owned render/debug item snapshots, builds draw sorting/shadow-caster collections and constructs prepared custom-material maps/texture bindings. Parameter-word buffer reuse removes one layer, not these other owned collections.
- Even a held frame emits a `ResponseBody::Frame` message. Its fresh encoder vector grows through 8/16/32/64-byte capacities: **four allocation/reallocation calls and 120 requested bytes**, matching the remaining `host.tick` category. The outer outbox capacity is retained; this message payload still needs a transport-to-Host buffer return path to become reusable.
- Eventful frames also allocate owned playback/resource/outcome records and encoded replies; report fanout clones records for each attached session. Client encoding and transferred input payloads are outside the warmed idle robot's Rust count.
- JavaScript still constructs status callbacks and some browser-returned state objects. WebGL/driver internals are outside both Rust counts and V8 JavaScript heap sampling.

The source sweep also identified paths absent or inactive in the robot corpus: scalar constraints rebuild ordered targets and restoration-map nodes; live particle emission builds death heaps and mesh emission tables; particle draws build instance vectors; CPU geometry demand builds owned source sets when that demand is requested; active debug rendering, texture-valued materials, picks/projections, asset streaming and errors have additional owned results. These are reviewed source findings, **not measured zero-allocation claims**. The searchable source inventory is retained in `target/allocation-sweep/allocation-site-inventory.txt`; cold setup/serialization/test matches are not automatically per-frame allocations.

The GL bridge also still performs repeated limit/state queries and error checks during draws; those calls and graphics submission waits are CPU costs even when they do not allocate. These measurements do not include GPU timer queries or establish performance on hardware graphics.

Allocation removal also does not eliminate the animation CPU scaling problem: generic callbacks still traverse binding/controller collections per component commit, and drivers retain boxed dynamic dispatch, typed-track downcasts and resource lookups. The large scalar case remains superlinear. The earlier safe/raw-pointer comparison found no material unsafe-specific advantage. Zero-allocation sampling into caller-owned pose storage and affected-binding indexes are more relevant next investigations than changing the final assignment to pointer syntax. Such work must preserve exact property coverage, source ownership, callbacks and invalidation before reuse.

## Reproduce

```sh
cargo build -p ipp-protocol --example profile_updates --release --features performance-experiment --locked
IPP_PROFILE_MODES=0,3,3,0 taskset -c 22 target/release/examples/profile_updates
IPP_PERFORMANCE_EXPERIMENT=1 node tools/build_browser.mjs render-expanded headless
node tools/build_gallery.mjs
LD_LIBRARY_PATH=/home/dev/.local/opt/ipp-browser-support/sysroot/usr/lib64 IPP_PROFILE_MODES=0,3,3,0 node tools/profile_robot.mjs
LD_LIBRARY_PATH=/home/dev/.local/opt/ipp-browser-support/sysroot/usr/lib64 IPP_PROFILE_MODES=0,3,3,0 IPP_PROFILE_PLAYING=1 node tools/profile_robot.mjs
LD_LIBRARY_PATH=/home/dev/.local/opt/ipp-browser-support/sysroot/usr/lib64 IPP_PROFILE_MODES=0,3 IPP_PROFILE_JS=1 node tools/profile_robot.mjs
```

Finish builds before timing and run native/browser timings separately. JS sampling is collected in separate windows through Chromium's heap sampler; it is a statistical estimate and includes diagnostic readback/Playwright activity. Filter to production frame stacks before comparing it, and do not present total sampled heap bytes as production frame allocations. Captures, inspection and JSON readback occur outside Rust profiling windows.

## Validation and review boundary

- Native A/B completes with zero-allocation assertions for every warmed mode-3 scalar/idle update and reused scalar decode; exact sampled scalar values are checked.
- 111 expanded library unit tests pass across core (42), HostSession (22), protocol (36) and GL renderer (11). Added tests check mutation-map ordering/replacement/spill, borrowed source-key ordering, decode capacity growth/reuse/error fences and all numeric std140 packing kinds. Controller deletion now explicitly asserts disappearance.
- Focused core integration tests exercise animation, dynamic properties, hierarchy, spatial behavior, skeleton/joint animation, overlays and shared resource lifecycle. Three failures reproduce with mode 0: `source_replacement_in_failed_batch_invalidates_binding` expects negative Transform scale to be rejected; `cycles_and_terminal_dependencies_retain_changes_and_recover_on_correction` expects a matrix after a cycle error; `uploads_precede_batches_and_old_assets_remain_immutable_through_staging` expects resident bytes `(94, 78)` while the snapshot reports `(94, 180)`. They are inherited baseline findings, not passing coverage. Logs preserve each failure. The remaining selected cases pass. Asset lifecycle/ownership checks were repeated after the final observation-dirty adjustment.
- `npm run test custom-materials` passes using the ordinary optimized build, with real generated-client transport, assets, color/frame assertions, sparse overlays, fallback and graphics-context recovery. Its maintained prerequisite pipeline also builds the relevant lean/expanded browser distributions.
- Final held-pose/JS and walking robot experiments pass all four mode blocks each. Their environment harness captures evidence and closes its browser, workers and server.
- Scoped expanded all-target Clippy passes across core, protocol, HostSession, renderer and WASM with `-D warnings -A dead-code`. The allow covers the inherited unused `WorldSimulationState::fault`; other warnings are denied. TypeScript checking, pinned source formatting/checking, repository checks, workspace policy (eight crates/76 resolved feature graphs), expanded/minimal WASM builds and `git diff --check` pass.

Exact commands and outputs are under `target/allocation-sweep/`: `final-unit-tests.log`, `core-final-tests.log`, `core-final-remaining.log`, `asset-final-recheck.log`, `baseline-hierarchy-failure.log`, `baseline-spatial-failure.log`, `clippy-final.log`, `custom-materials-integration.log`, `robot-final-held.log`, `robot-final-moving.log`, `format.log`, `format-check.log`, `typescript-final.log`, `check-repo.log`, and `check-workspace.log`. The earlier baseline skeleton failure is in `target/performance/baseline-skeleton-failure.log`. `final-build-sha256.txt` identifies measured artifacts. `sweep-only.patch` and `sweep-manifest.json` distinguish this sweep from the earlier experiment/inherited snapshot.

The worktree is the review deliverable; no commit, merge, push or full regression was requested or performed. The safe allocation changes still need integration review against the primary checkout's concurrent changes. Modes 1/2 are experimental callback-bypassing controls, not candidate production behavior. No new third-party runtime dependency or accepted architectural decision is introduced. This pass does not certify every possible IPP scene as allocation-free, and the full robot has not yet reached zero.
