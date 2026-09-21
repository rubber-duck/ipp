# Blender stress benchmark

A reproducible Blender simulation, standard export, generated-client import and saved World replay exercise the actual IPP runtime. This benchmark is opt-in and is never part of regression selections. See [native measurements](native-results.md) for historical results; new runs record their own source and fixture identity.

## Scene

The full preset contains a seeded 100 × 100 grid of individually animated rigid cubes, a flat collision plane and 241 samples covering ten seconds at 24 Hz. Blender's Bullet simulation supplies the samples. The generator saves both `physics-source.blend`, with the original simulation, and `benchmark.blend`, with the exported baked actions.

Twenty local lights are parented across the falling grid: sixteen point lights and four spot lights, with animated intensity/color and four shadow casters. An orbiting camera, two twelve-level attachment chains and eight shared PBR materials exercise composed transforms, light selection, shadows and repeated mesh use. Opposite sides emit 2,000 cube particles each over ten seconds, one through baked physics and one through native simulation. Lifetimes are at most three seconds; 2,000 is the emission count, not simultaneous live occupancy. A separate fading alpha sprite emitter exercises transparent instance sorting.

Four low-poly humanoids use a real generated Rigify human rig as the bake source. The authoring rig has 222 bones; the exported skin uses a 31-bone deformation subset within the current 32-joint runtime limit. The walk is authored on Rigify FK controls and baked into the compact skeleton. This is a functional rig/skin workload, not a photorealistic character asset. The small preset retains all subsystem types but uses 64 cubes and 200 particles per side.

Fixture version 2 adds 32 deforming lattice panels (four in smoke), with 425 source vertices each and shared immutable endpoints across four appearance variants. Shape-key weights animate throughout the ten-second timeline. Surfaces include unlit/PBR factors and textures, a multi-material mesh split, and an affine parent that introduces shear. The four walkers also animate a silhouette shape before skinning.

The [Blender generator](../blender/stress_scene.py) authors supported export features. The shared [runtime supplement](features.ts) adds features without Blender mappings through the generated client, identically for native and browser Hosts. Native bundles retain the supplemental entities, animation assets and shader definitions in the saved World. Browser imports add them before warming. The `.blend` file alone therefore does not contain the complete runtime workload.

| Area | Workload and observations |
| --- | --- |
| Animation | Imported transform/quaternion/joint/vertex-pose clips; linear, stepped and Bézier scalar curves; weighted and additive evaluation; repeating short drivers on a ten-second controller; animated custom vector/scalar parameters and camera projection |
| Deformation | Shared rigid mesh poses, material splits, affine composition and pose-before-skin; independent Blender weight samples and native bounds enclosure checks; browser closeups vary only pose weights while other inputs remain held |
| Constraints/hierarchy | Existing deep object chains and parented lights, a joint attachment and joint-pair picking pill, scalar linear constraint, and LookAt following an animated target; projection queries verify the final constrained camera |
| Materials | PBR, unlit factor, `BaseColorTexture`, separate `UnlitTexture`, textured custom opaque/cutout/blended surfaces, animated custom vertex displacement with conservative bounds, and custom instanced particle shading |
| Geometry/cameras | Generated and authored bounds; inline and immutable-resource box/sphere/pill/compound geometry; filled and outline visualization; perspective and orthographic cameras; independent expected picking hits for all four shape kinds |
| Particles | Existing baked/native mesh and alpha-sprite effects, plus moving point/box/sphere emitters, local/World simulation, textured alpha/additive sprites, velocity alignment, and custom particle meshes |

The [shared feature checks](feature-checks.ts) require every component in the compiled production registry to appear in effective scene state. Adding a component deliberately requires revisiting coverage. They also check independently expected animation values, camera projection, picking and imported Blender pose weights. Native runs retain `bundle/features.json`; browser reports retain these checks and deformation image differences in `profile.json`. The native profiler checks extremal and interior Blender vertices against the final mesh bounds, including the skinned result; conservative enclosure is not an exact skinned-surface comparison.

## Run

Use the pinned [toolchain](../../docs/development/building.md) and [Blender environment](../../docs/development/blender.md). Native runs require an explicit EGL/GLES library directory. On the development VM use `/lib64`; browser runs use the existing `ipp-browser-env` wrapper for private browser libraries.

```sh
python tools/ipp.py benchmark native --preset smoke --egl-dir /lib64
python tools/ipp.py benchmark native --preset full --egl-dir /lib64
ipp-browser-env python tools/ipp.py benchmark browser --preset smoke --frames 20
```

Python owns prerequisites, builds, scene generation, export, process lifetime and reports. Native execution imports through the real WebSocket Host with its matching generated client. Browser execution imports through the actual worker/WASM host. Both verify independent Blender probes and completed frames. The smoke preset retains all subsystem types.

Products live in `target/performance-build/native`, `native-instrumented` and `browser`. Scene sources default to `target/stress-benchmark/<preset>`; reports and saved bundles default to `target/stress-benchmark/<backend>-<preset>`. `--scene-dir`, `--bundle-dir` and `--output` select explicit locations. Browser bundles must be in `OUTPUT/bundle`.

Use `--reuse-scene`, `--reuse-import` and `--reuse-build` to reuse explicitly prepared artifacts; only reuse a saved World with a matching contract. Reused native binaries must match their recorded digest. Run identity retains the original build identity even when a previous binary is selected. `--build-only` prepares the selected host without creating a scene.

Regenerate older fixtures without `--reuse-scene` when moving to version 2. Reimport after changes to the runtime supplement. Baseline comparisons must use the same fixture version and supplement: version 2 intentionally adds work and is not directly comparable to the earlier fixture.

```sh
python tools/ipp.py benchmark native --preset full --egl-dir /lib64 \
  --reuse-scene --reuse-import --frames 120 --output target/stress-benchmark/native-full
python tools/ipp.py benchmark native --preset full --egl-dir /lib64 \
  --instrumented --reuse-scene --reuse-import --frames 30
```

Ordinary native release timing and profiled allocation/stage timing are separate products. The `profiling` feature does not select different runtime paths. `--group` controls browser controller grouping. `IPP_STRESS_JS=1` adds a separate browser allocation sample. `--plan --json` shows prerequisites without executing them.

The standalone unit-weight curve microbenchmark is omitted when a scene contains weighted, additive or dynamic-property drivers. Its typed-reference comparison does not represent those operators. Full-frame and allocation measurements always retain every driver, including the new composed operators.

## Hardware WebGL and addon profiles

Set `IPP_BROWSER_ANGLE=vulkan` or `IPP_BROWSER_ANGLE=gl-egl` to use full Chromium with the selected ANGLE backend. The environment probe and browser scenarios share these launch options. The probe rejects software or unavailable renderer identities; the stress scenario also checks the actual worker renderer on every completed capture. With the variable unset, correctness runs retain their software-capable default and make no hardware claim.

```sh
IPP_BROWSER_ANGLE=vulkan ipp-browser-env node tools/build/probe-browser.mjs
IPP_BROWSER_ANGLE=vulkan IPP_STRESS_CPU=1 ipp-browser-env \
  python tools/ipp.py benchmark browser --preset full --frames 60 --reuse-scene
```

Capture metadata records unmasked renderer/vendor, and browser evidence records Chromium version, launch options and build hashes. `IPP_STRESS_CPU=1` saves separate held/moving worker CPU samples alongside ordinary timing and allocation windows. CPU samples include JavaScript, WASM, browser API calls and idle time; they do not measure GPU execution. Keep scene, camera, build, hardware backend and sampling settings constant across comparisons. Do not run competing benchmarks during timing windows.

Every run records load stages in `loading.json`: bundle fetch, World restoration, inspection, supplemental feature creation, controller creation, asset readiness and initial playback setup. `IPP_STRESS_PERSISTENCE=1` additionally saves the freshly restored World, destroys it, and reloads that save before running the usual state and image assertions. Save/load timings include transfer and Host processing; resource readiness is measured separately. `IPP_STRESS_LOAD_CPU=1` samples the worker during the entire setup and retains the profile in the same artifact. Use a separate run without sampling for ordinary timings. A build with `CARGO_PROFILE_RELEASE_SMALL_STRIP=none` retains Rust names for CPU profile interpretation.

Profile the standard addon exporter independently of browser loading:

```sh
blender --background target/stress-benchmark/smoke/benchmark.blend \
  --python-exit-code 1 --python integrations/blender/export_scene.py -- \
  target/stress-benchmark/profile-export --profile target/stress-benchmark/export.prof
python -m pstats target/stress-benchmark/export.prof
```

The exporter reports `exportSeconds` and whether profiling was enabled. Omit `--profile` for ordinary timings; sampling instrumentation adds overhead. The profile includes immutable asset publication through the disk callback. It excludes Blender startup and final snapshot/catalog serialization. Background extraction does not establish GUI responsiveness.

## Interior camera views

```sh
python tools/ipp.py benchmark native --preset full --egl-dir /lib64 \
  --culling-views --reuse-scene --reuse-import --frames 60
```

The overview camera and two fixed interior views share projection and far plane. A fourth interior view uses a 30-metre far plane as a separate variable. Each station reloads the same saved World and renderer, warms 200 advancing frames and keeps every animation driver active, including the original camera. This avoids live-particle history drift between stations.

At each completed pose the flat scan and BVH must return identical candidate entities and byte-identical captures. Rejected entities also pass an independent exact-geometry visibility check. Reports include surface/shadow draw counts, rendered and isolated update timings, query latency, candidates, unknown bounds and camera parameters. Query timings exclude geometry evaluation/refit. Camera culling is conservative frustum rejection, not occlusion culling.

## Draw and spatial comparisons

`--geometry-index flat|bvh` selects the index before loading. Compare ordinary release runs in alternating order. Full frame costs include bounds evaluation and tree maintenance as well as render queries.

`--draw-sweep` intercepts native GLES calls in the benchmark host. Controls skip indexed draws, submit a fraction, discard rasterization or skip submission state calls while retaining engine traversal. Normal captures before and after interventions must match. The controls intentionally alter their own images; they are diagnostic experiments, not production optimizations.

`--render-profile` enters a named `profile_window` after warming the moving scene. Build with symbols and frame pointers when using a system sampling profiler:

```sh
RUSTFLAGS='-C force-frame-pointers=yes' CARGO_PROFILE_RELEASE_DEBUG=1 \
  python tools/ipp.py benchmark native --build-only
perf record -e cpu-clock:u -F 999 --call-graph fp -o target/profile.data -- \
  python tools/ipp.py benchmark native --preset full --egl-dir /lib64 \
  --reuse-build --reuse-scene --reuse-import --render-profile --frames 240
```

Filter sampled stacks to `profile_window` to exclude startup/import. Keep the measured executable with its trace. `--culling-views`, `--draw-sweep` and `--render-profile` are mutually exclusive.

## Evidence and limits

Native results include CSV frame/stage timing, allocation counts where enabled, captures, environment, original Blender probes and source/build/fixture identity. Browser results include state, captures, profile windows and failure artifacts under `target/integration-artifacts/stress`; `node tools/analyze_stress.mjs <profile.json>` summarizes them.

Warmed allocation counts cover Rust allocation/reallocation requests inside the measured windows, not loading, growth, JavaScript or graphics-driver memory. Timing results depend on hardware, camera, retained live particles and shared-host load. Preserve these inputs when comparing regressions. The [small action-sampling correctness test](../blender/action_sampling_check.py) remains in the normal Blender suite; it does not generate or run the stress scene.

Coverage means all current scene component types and the paths above, not every engine operation or permutation. Resource replacement, overlays, World/session lifecycle, context loss, malformed protocol input and transport backpressure remain in their focused integration suites; the steady-state benchmark does not inject that churn into every frame. Planned features such as IK, IBL and MSAA are not represented as implemented workloads. Expanding the benchmark does not add it to regression selections.
