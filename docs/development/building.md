# Building and Developing IPP

[Workspace policy](../architecture/rust-workspace.md) · [Workflow](workflow.md) · [Build strategy](../plans/runtime-and-rendering.md#build-composition-and-size)

## Toolchain and scope

Install Rustup, Node from [.node-version](../../.node-version), and Python 3.14 for repository tools. [rust-toolchain.toml](../../rust-toolchain.toml) selects Rust, rustfmt, Clippy and `wasm32-unknown-unknown`; [Cargo.toml](../../Cargo.toml) and [Cargo.lock](../../Cargo.lock) own workspace settings and dependency pins. Blender's bundled Python and addon wheels have a separate [environment](blender.md).

The maintained guides cover [headless hosts](headless-hosts.md), [React](react.md), [rendering](rendering.md), [animation](animation.md), [cameras/picking](cameras-and-picking.md), [skinning](skeletal-skinning.md), [mesh poses](mesh-poses.md) and [Blender](blender.md). Source guides describe [custom materials](../../crates/ipp-render-gl/README.md) and [particles](../../crates/ipp-core/src/world/systems/particles/README.md). Check their limitations and maintained suites before treating an architectural direction as implemented behavior.

Current support differs from the broader architectural direction:

| Area | Delivered | Intended or outside current support |
| --- | --- | --- |
| Evaluation | Property/joint animation, reversible playback and runtime crossfades, affine object and joint parenting, scalar `LinearDriver`, terminal object LookAt | Other basic constraints, joint-local constraints and iterative solving |
| Rendering | Unlit/PBR and custom materials, direct lights, uniform ambient fill, shared spotlight shadow allocation, WebGL/GLES and context recovery | Image-based lighting, MSAA, full PBR texture inputs and visibility/quality-driven residency |
| Authoring | React declarations/assets/animation, including adoption of existing entities by declared symbolic id when a root mounts again after reconnect; Blender object/bone parenting, a single relative shape key and reusable active/stashed clips | React remote refs; general Blender shader graphs or lossless NLA/constraint translation |
| Data and hosts | Shared Worlds, reference-only persistence, browser HTTP and native filesystem input | Asset bundles, native HTTP/IPC source adapters and independent simultaneous GL contexts over one catalog |

Particles provide CPU emission/cache playback and instanced presentation; mesh poses and skeletal animation provide separate deformation paths. [Surfaces](../../examples/surface-terminal/README.md) present Canvas content (quadratic text and drawings, RGBA bitmaps and clipping) on transformed planes. Text supports basic labels and client-shaped glyph runs; the offline SVG converter accepts a documented subset. Surfaces may opt into [whole-Surface texture caching](../architecture/rendering.md#optional-surface-texture-caching) that keeps nearby, focused and interacted-with Surfaces direct and lowers cache resolution and refresh with camera distance; the [policy](../../crates/ipp-core/src/world/systems/surface/cache_policy.rs) and [renderer cache](../../crates/ipp-render-gl/src/services/render/surface_cache.rs) own the numbers, and the [GUI demo](../../examples/world-gallery/worlds/gui/README.md) compares cached and direct presentation. [Exporter scope](../../integrations/blender/ipp_blender/EXPORTER.md) owns supported Blender combinations. Beads tracks remaining work; maintained suites establish behavior in their tested environment.

The GUI capability provides controls as ordinary entities in the core entity tree, control values and scroll state as ordinary component fields, headless layout, Canvas presentation of skin shape materials through retained triangle batches with shared glyph-atlas text, routed pointer/keyboard/text interaction including provisional composition, nested ScrollView wheel and drag scrolling with skinnable scroll bars, VirtualLists that scroll by item count and declare only their wanted item range, actions, field subscriptions and focus/pointer queries, and the `@ipp/react/gui` declarations with client text/clipboard/IME bridges. Screen-reader bridging and the other [out-of-scope items](../architecture/gui.md#ownership-and-scope) remain future work.

## Crates and features

[Workspace architecture](../architecture/rust-workspace.md#compile-time-composition) owns crate responsibilities and the three build axes. Plain `cargo check` selects the default workspace members; `--workspace` includes rendering, WASM and tooling. Every build compiles every capability; a World excludes one through its System selection.

Default features are empty everywhere, so a plain build is the production build; it logs, answers statistics requests and reports panics through its log sink. The only features are `instrumentation` (renderer testing overrides, the GLES test host's testing controls, the counting allocator, profiling timers and benchmark exports), forwarded by the hosts; `render` on the WASM host, which links the WebGL renderer; and `checked-invariants`, core's own test oracle. The native host keeps the renderer as a dev-dependency of its profiling example and its `gles_host` testing host, and always includes its WebSocket transport. Exact declarations live in the [core](../../crates/ipp-core/Cargo.toml), [renderer](../../crates/ipp-render-gl/Cargo.toml), [native](../../crates/ipp-server/Cargo.toml) and [WASM](../../crates/ipp-wasm/Cargo.toml) manifests.

[Browser distributions](../../tools/pipeline/profiles.json) are named by their axis values and exist only where something consumes them: `headless` (no renderer, production), `render` (WebGL, production) and `render-instrumentation` (WebGL with test controls and profiling). Scenarios run `render` unless they use a test control or the profiler; the GLES test host likewise has a normal `gles-host` and a `gles-host-instrumentation` product. For example:

```sh
python tools/ipp.py build browser:headless browser:render
python tools/ipp.py check contracts contract-identities browser-identities
```

The builder compiles each distribution's runtime once, reads the contract from that runtime, generates the matching client and verifies the final runtime, client and assembled host. Each host target has one contract: `check:contracts` verifies the native and WASM contracts and their generated clients, and the identity checks prove that instrumentation and the renderer never change a target's contract. Contract changes require regenerated clients and fixtures. At connection the Host announces its compatibility hash and serves its contract on request; a generated client refuses a Host whose hash differs from its own ([client guide](../../packages/ipp-client/README.md#build-and-connect)).

## Diagnostic output

Follow the [logging policy](../architecture/runtime.md#diagnostic-logging) and [levels](../../crates/ipp-core/src/diagnostics.rs).

Native: `IPP_LOG=debug cargo run -p ipp-server --locked`. Levels: `error`, `warn`, `info` (default), `debug`, `trace`, `off`.

Every browser distribution logs: set `logLevel` in connection options or `IppCanvas.runtime`. Every build also reports a panic's message and source location through the log sink before aborting.

Every build keeps renderer, resource, ingress and lifecycle statistics, read on demand through [`@ipp/client/diagnostics`](../../packages/ipp-client/README.md#physical-presentation). `@ipp/client/testing` sends loss simulation and budget overrides that only `instrumentation` builds honour; against any other build each control throws at the call. Whether a browser distribution ships the worker side of those controls and the profiler module is decided by its build configuration in the [assembler](../../packages/ipp-client/tools/assemble.mjs), never by probing the runtime's exports.

## Pipeline setup and commands

[tools/ipp.py](../../tools/ipp.py) is the maintained entry point. Python owns the execution graph, prerequisites, subprocess cleanup and evidence. Node operations perform bundling and target WASM execution; Cargo compiles Rust. Importing the planner and requesting help or a plan uses only the Python standard library.

Install the Python minor version in [.python-version](../../.python-version), Node from [.node-version](../../.node-version), and Rustup. Prepare only the environments you need:

```sh
python tools/ipp.py setup node
python tools/ipp.py setup python
python tools/ipp.py setup rust
python tools/ipp.py doctor
python tools/ipp.py setup browser --with-deps
python tools/ipp.py doctor --for cameras
```

Use `python3` if that is your platform's executable name. Python tools are installed in `.venv` and resolved directly; shell activation is unnecessary. The optional Blender setup uses its separate pinned release and wheel lock. `setup certificates --install-trust` explicitly installs development certificate trust. Ordinary builds/checks never install dependencies or change trust. Beads/Dolt remain optional for compilation; `doctor --coordination` checks their installed versions without opening the task database.

Every public command supports `--help`, `--plan` and `--json`; build/test/check/regression also expose `--list`. Plans explain prerequisites and environments before executing anything. JSON output uses stdout; progress and child output use stderr. `--live` streams child output while retaining logs. Missing required environments stop execution before expensive builds and produce a failed prerequisite report.

```sh
python tools/ipp.py build gallery
python tools/ipp.py build gallery-site
python tools/ipp.py build browser:headless browser:render
python tools/ipp.py check typecheck
python tools/ipp.py test cameras worlds
python tools/ipp.py test cameras --plan --json
python tools/ipp.py check --changed --suite worlds
python tools/ipp.py dev gallery
python tools/ipp.py dev headless-client ws://127.0.0.1:9231
```

`check --changed` examines staged, unstaged and untracked files; `--base REF` includes committed changes since the merge base. It explains conservative selections and requires explicit `--suite` coverage for paths whose runtime impact it cannot determine. It never infers full regression. Typechecking prepares its actual target client. `dev NAME --build` prepares without launching. Examples use the same products as tests; application builds exclude test-only bundles.

`build gallery-site` writes a standalone release to `target/gallery-site/`: HTML, minified application/styles/runtime modules, release-small WASM, gallery assets and license notices. Relative URLs support both a domain root and a project subdirectory. `build-report.json` records file hashes and raw/gzip size estimates; published files remain uncompressed for the host to negotiate HTTP compression. The complete GUI font remains available for editable text.

`test gallery-site` builds the release and exercises every gallery page through HTTP gzip, worker/WASM and WebGL at root and project URLs, retaining frame and network evidence under `target/integration-artifacts/gallery-site/`. It does not test asset reacquisition after explicit eviction: the browser provider currently requires strong recovery validators, while Pages may return weak ETags for compressed responses.

The [Pages workflow](../../.github/workflows/gallery-pages.yml) builds, tests and uploads the site in one job and runs the retained GUI and Surface cache browser suites in a separate bounded, cached job; it deploys on pushes to `main` or manual runs from `main` only after both jobs pass, because the published gallery renders through the retained GUI path. Set the repository's **Settings → Pages → Build and deployment → Source** to **GitHub Actions** before the first deployment. Building locally does not publish the site.

Gallery builds require Blender 5.2 and Chromium (`python tools/ipp.py setup browser --with-deps`). Local development can install the pinned Blender archive with `python tools/ipp.py setup blender`; Pages CI installs the Blender Foundation Snap and verifies its 5.2 version. The build exports the maintained packed scenes into `target/gallery-platformer-assets/` and `target/gallery-gui-assets/projector/`; Platformer World serialization uses the matching worker/WASM contract. No KayKit archive download or projector texture rebake is needed. Runtime exports and authoring previews stay under ignored `target/`, while editable Blender/SVG sources and licenses remain in Git.

Gallery, Surface and GUI builds prepare [shared fonts](../../assets/fonts/README.md) through the `font-assets` prerequisite. Clean local and GitHub Actions builds download the pinned sources automatically and verify their SHA-256 checksums; subsequent builds reuse the verified `target/font-sources/` cache. `python tools/ipp.py build font-assets` prepares that cache and the shared runtime font directly.

The [catalog](../../tools/pipeline/catalog.py) owns products/checks, [profiles](../../tools/pipeline/profiles.json) own target selections, [suite groups](../../tools/pipeline/suites.json) select tests, and [test inputs](../../tools/pipeline/test-inputs.json) own each test's prerequisites. A test shared by multiple suites keeps the same declared inputs. The catalog check rejects a test whose source, or a module it imports by relative path, literally names a build product directory it does not depend on; a suite command that runs only part of a shared test file lists the products that part never reaches under `partitionExcludes`. The [check](../../tools/pipeline/product_reads.py) sees only literal `target/` paths. Browser profiles build independently. Overlapping selections run each prerequisite and test once.

## Formatting

Pinned tools are Biome/Prettier in [package.json](../../package.json), Ruff/Mypy in [requirements-dev.txt](../../requirements-dev.txt), and rustfmt in [rust-toolchain.toml](../../rust-toolchain.toml). Mypy checks the Python pipeline's annotations. Formatting never applies lint fixes.

```sh
npm run format
npm run format:check
python tools/ipp.py format --language md docs/architecture.md
python tools/ipp.py format --language md --check docs/architecture.md
python tools/ipp.py check python-types
```

The npm formatting shortcuts invoke the same Python pipeline. JavaScript/TypeScript, Python and Markdown formatting selects this checkout's tracked and unignored untracked files through Git, without traversing nested repositories or worktrees. Rust uses Cargo workspace discovery. `--language js|python|rust|md` narrows selection; explicit paths require one language and narrow the same file selection. Generated output and dependencies remain excluded. Format maintained generator templates, then regenerate through the relevant build.

[Prettier](../../.prettierrc.json) preserves unwrapped prose and code-block layout. [Biome](../../biome.json) and [Ruff](../../ruff.toml) own language styles. [Rustfmt](../../rustfmt.toml) preserves manually supplied blank lines between definitions and logical steps; review that spacing, including macros. Blender source retains its embedded-Python target override.

## Evidence and execution

Every run writes a unique `target/pipeline/runs/run-*/summary.json`, incremental status and complete child logs. Build steps also write manifests with source-content identity, commands, tool/environment identity, dependency manifests, and output hashes. Reports describe the exact products observed in that run. Browser products retain separate build reports, and generation verifies that the client matches the contract of the shipped runtime before publication.

The executor serializes writers within a checkout using an OS lock, released even after a crash. Use separate source worktrees for concurrent pipeline execution. It owns child processes, reports timeouts/cancellation, blocks dependents after failure and continues independent work unless `--fail-fast` is selected, which stops new starts and lets running steps finish. Interactive development servers stream their URL and stay owned until stopped. A source change during execution is recorded explicitly; that run cannot establish one unchanged source snapshot.

Steps run concurrently over the dependency graph: a step starts once all its prerequisites passed. The [runner](../../tools/pipeline/runner.py) derives every width from the cores available to the process and records the widths and core counts as `scheduler` in `summary.json`. Steps declaring the `rust` requirement run one at a time because Cargo steps share one build directory and already use every core, so every step that drives Cargo must declare `rust`; a step that declares it without running Cargo only waits longer. Steps declaring `browser` share a smaller browser width, which bounds how many separate Chromium instances run at once; browser scenarios launch their own. `--live` and plans containing interactive steps run serially. Reports keep steps in plan order; the console prints a start and a finish line per step and one combined line of running steps every 30 seconds.

Suites in the [suite registry](../../tools/pipeline/suites.json) and native GLES checks in [gles.json](../../tools/pipeline/gles.json) declare shared source ownership through `sourceRoots`: a root ending in `/` owns a directory, any other root one file. Changed-file selection includes every matching suite and check, and a changed test file declared by a suite selects itself, in addition to the conservative area rules; keep these roots aligned when moving shared implementation or test helpers. Files without a precise mapping still require an explicit suite selection.

Each invocation rebuilds declared prerequisites through the underlying incremental tools. There is no implicit result cache or build-skipping flag. New reports do not certify previous passing checks after edits. Retries read unfinished IDs, reuse the recorded browser device and EGL directory unless overridden, resolve the current catalog and never execute commands stored in a report:

```sh
python tools/ipp.py retry target/pipeline/runs/run-EXAMPLE/summary.json
python tools/ipp.py retry target/pipeline/runs/run-EXAMPLE/summary.json --suite cameras
```

Preserve applicable prior reports and reuse evidence only while its source, configuration and environment remain valid. Prerequisite reports and compile success do not prove runtime or rendering behavior. Maintained scenarios retain outcomes, frame observations and failure artifacts under `target/integration-artifacts/`.

### Regression entry point

`python tools/ipp.py regression` (also `npm run regression`) runs the bounded **core** gate: repository/catalog/workspace checks, formatting, Python/TypeScript checks, pipeline tests, default Rust tests/Clippy, generated-client tests and real native WebSocket integration. It requires the normal Rust/Node/Python development tools, with no browser, WASM, Blender or GLES environment. Cold builds still pay compilation costs.

Merge-to-main and push requests require core plus coverage for affected subsystems and callers. A plain commit, handoff or working on `main` does not trigger an additional regression pass. The `--full` flag selects complete regression when explicitly requested; it includes every maintained check, suite, target distribution and native GLES scenario. Stress benchmarks remain separate under `benchmark`.

```sh
python tools/ipp.py regression
python tools/ipp.py regression --list
python tools/ipp.py regression --group gui --group rendering --plan
python tools/ipp.py regression --group gui --group gallery
python tools/ipp.py regression --only check:repository --suite cameras
python tools/ipp.py regression --retry target/pipeline/runs/run-EXAMPLE/summary.json --group gui
LIBGL_ALWAYS_SOFTWARE=1 python tools/ipp.py regression --full --egl-dir /usr/lib/x86_64-linux-gnu
```

The pipeline selects the browser device for every child and for the preflight, whatever `IPP_BROWSER_ANGLE` the invoking shell sets. By default a run with browser steps uses hardware when it is available: its browser preflight tries ANGLE on Vulkan, then GL through EGL, and keeps the first that reports a hardware renderer; otherwise browser steps render in software (SwiftShader through ANGLE). A machine without a usable GPU, such as hosted CI, therefore runs on software after two short failed probes. `--software` forces software rendering. `--hardware vulkan` or `--hardware gl-egl` forces that backend and fails the preflight, and the hardware-checking scenarios, when Chromium reports a software renderer. The two options are mutually exclusive and apply to `regression`, `test`, `check` and `retry`; `doctor` reports the device a default run would select. The first console line names the device and why it was chosen. `summary.json` records it under `browser`: `device`, `selection` (`automatic`, `requested` or `inherited`), `reason` and, when detected, each probe `attempts`; `environmentSelection.browserDevice` carries the device into every step manifest. A retry keeps the browser device and EGL directory of the report it retries instead of detecting again; an explicit device option or `--egl-dir` overrides them. `benchmark` defaults to `--hardware vulkan` and never falls back to software.

```sh
python tools/ipp.py regression --group rendering --software
```

Repeatable `--group` adds on-demand coverage to core, using the existing suites and deduplicating shared tests and prerequisites. These groups select coverage by area; their real scenarios may require other environments for fixtures or participating runtimes. Inspect `--plan` before preparing an environment.

| Group | Run when changing |
| --- | --- |
| `runtime` | World state, lifecycle, persistence, animation, hierarchy, command streaming or React reconciliation |
| `browser` | Worker/WASM transport or browser lifecycle |
| `rendering` | WebGL, cameras, geometry, materials, lights, deformation or resource recovery |
| `gui` | Surface/GUI layout, input, retained rendering or cache correctness |
| `gallery` | Published gallery, demo behavior or application assets |
| `blender` | Addon packaging, export, streaming or Blender integration |
| `gles` | Native GL device/presentation or shared rendering behavior affecting GLES |
| `matrix` | Instrumentation or renderer features, target contracts, generation, dependencies or distribution composition: all-features Clippy, Rust tests and WASM builds, the production debug WASM, both target contracts and every browser distribution with its contract identity |
| `scaling` | Bulk mutation, hierarchy, restoration or command-streaming complexity |

Select the relevant combination or narrower named suites/checks; shared renderer changes generally require both browser rendering and GLES evidence. The [catalog](../../tools/pipeline/catalog.py) owns group membership and rejects unassigned new checks/suites. Core is an explicit selection so new expensive suites enter full regression and an on-demand group without silently expanding the routine gate.

`--only`/`--suite` without a group runs focused coverage. Retries run unfinished steps plus explicitly added groups/suites/checks, without adding core automatically; `--full` cannot be combined with `--retry`. Reports distinguish core, core plus named groups, focused partial coverage and full regression. A passing core report does not establish full regression coverage. Missing selected environments fail prerequisites rather than silently omitting coverage.

For a smaller tooling or native-only selection, use named checks and suites directly:

```sh
python tools/ipp.py check repository catalog format-python python-types --suite runner
python tools/ipp.py check workspace clippy-default clippy-all-features --suite runner
```

`--egl-dir` (default `IPP_EGL_LIBRARY_DIR`) selects actual EGL/GLES libraries; the executor exports the selection to every child as `IPP_EGL_LIBRARY_DIR` and records it in the report's environment selection. `NODE_BIN`, `BLENDER_BIN` and the invoking Python interpreter select executables; the executor passes the same selections to child harnesses. A Blender release installed through setup is resolved from `target/tools`. Apart from the device selection, the browser inherits its configured environment; use the [Blender/browser environment guide](blender.md) on hosts with private library wrappers.

The [Pages workflow](../../.github/workflows/gallery-pages.yml) invokes the `gallery-site` browser suite and the `retained-gui` and `surface-cache` browser suites through this CLI in separate jobs and uploads their evidence; the retained GUI job fails visibly when its browser environment is missing. Pipeline tests validate its commands against the current catalog. Regression commands and groups remain available for local use; routine pushes do not run them as separate GitHub Actions jobs. Local validation does not claim that hosted jobs ran. Extend the maintained real native WebSocket, browser worker/WASM/WebGL and GLES scenarios when changing participating behavior; keep scenario intent separate from process setup.

## Release and size experiments

Build by package, target and axis features; workspace unification can hide production-build costs.

```sh
# Production native host
cargo build -p ipp-server --release --locked

# Release-small production WASM hosts, headless and rendered
cargo build -p ipp-wasm --target wasm32-unknown-unknown --profile release-small --locked
cargo build -p ipp-wasm --target wasm32-unknown-unknown --profile release-small --features render --locked

# Inspect actual production dependencies and enabled features
cargo tree -p ipp-wasm --target wasm32-unknown-unknown --features render --edges normal,build --locked
```

`release-small` is defined in [Cargo.toml](../../Cargo.toml). Compare meaningful workloads against normal release, reporting raw/compressed WASM separately from JavaScript, shaders and native shims. Use `python tools/ipp.py measure <artifact> ...` for raw size, deterministic gzip size and SHA-256. Each browser distribution ships its `release-small` build unchanged as `target/browser-build/<distribution>/runtime.wasm`, and its `build-report.json` lists the JavaScript it emits; shaders are embedded in the WASM. Count shared files once and record toolchain/features; empty modules are not runtime size baselines.

Stable remains mandatory for normal builds. Add a pinned dated nightly only for concrete Miri/sanitizer verification or measured size experiments; no empty nightly jobs or unstable baseline dependency.

## Opt-in performance scenes

`python tools/ipp.py benchmark native|browser` runs the maintained [Blender stress scene](../../tests/performance/stress.md). Native runs accept `--egl-dir`; `--preset full` selects 10,000 animated cubes. Timing runs measure the normal build; counters require the separate `--instrumented` native build, and the browser stress scene and the GUI stress core profile read the `render-instrumentation` profiler. `python tools/ipp.py benchmark browser --scene retained-gui` compares analytic and retained Surface text over `--frames` streaming updates, optionally with the terminal panels cached (`--surface-cache`), and rejects stress-scene options; the [retained rendering guide](../../tests/performance/retained-gui.md) describes its self-identifying reports and the physical-device procedure. Benchmarks and performance build products are excluded from regression, including `--full`.
