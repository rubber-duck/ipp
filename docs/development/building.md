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
| Authoring | React overlays/assets/animation; Blender object/bone parenting, a single relative shape key and reusable active/stashed clips | React remote refs/session reattachment; general Blender shader graphs or lossless NLA/constraint translation |
| Data and hosts | Shared Worlds, reference-only persistence, browser HTTP, native filesystem and optional ZIP input | Asset bundles, native HTTP/IPC source adapters and independent simultaneous GL contexts over one catalog |

Optional particles provide CPU emission/cache playback and instanced presentation; mesh poses and skeletal animation provide separate deformation paths. Optional [Surfaces](../../examples/surface-terminal/README.md) provide quadratic text and drawings, RGBA bitmaps, item animation and clipping on transformed planes. Text supports basic labels and client-shaped glyph runs; the offline SVG converter accepts a documented subset. [Exporter scope](../../integrations/blender/ipp_blender/EXPORTER.md) owns supported Blender combinations. Beads tracks remaining work; maintained suites establish behavior in their tested environment.

The optional `gui` capability provides GuiRoot trees with fenced node handles, revision-gated committed control values, single Surface content ownership, headless layout, Surface presentation of skin shape materials through retained triangle batches with shared glyph-atlas text, routed pointer/keyboard/text interaction including provisional composition, generated client edits and inspection, semantic snapshots/actions and the `@ipp/react/gui` declarations with client text/clipboard/IME bridges. Screen-reader bridging and the other [out-of-scope items](../architecture/gui.md#ownership-and-scope) remain future work, as does optional [whole-Surface texture caching](../architecture/rendering.md#optional-surface-texture-caching).

## Crates and features

[Workspace architecture](../architecture/rust-workspace.md) owns crate responsibilities and the baseline. Plain `cargo check` selects the default workspace members; `--workspace` includes rendering, WASM and tooling. Build individual packages with explicit features when checking lean distributions.

Core/protocol/session/hosts default to `builtin-assets`; renderer/build tools have empty defaults. Use `--no-default-features` to omit defaults. Scene selections include `skeletal-animation`, `mesh-poses`, `particles`, `surfaces`, `shadows` and `builtin-assets`. Hosts forward the relevant selections; `shadows` enables host rendering. Platform/tooling selections include `render`, native `websocket`, core `zip-data-source`, `diagnostics` and `schema-export`. Exact declarations live in the [core](../../crates/ipp-core/Cargo.toml), [renderer](../../crates/ipp-render-gl/Cargo.toml), [native](../../crates/ipp-server/Cargo.toml) and [WASM](../../crates/ipp-wasm/Cargo.toml) manifests.

[Browser configurations](../../tools/pipeline/profiles.json) own distribution names and flags. For example:

```sh
python tools/ipp.py build browser:headless browser:render-particles
python tools/ipp.py check contracts:baseline contracts:expanded
```

The builder executes target exports, generates matching clients and verifies the final runtime contract and compiled-out payloads. `TARGET.features` and `CAPABILITIES` describe the receiving target. Contract changes require regenerated clients and fixtures; GPU selection/diagnostics do not change otherwise matching authored-state contracts. Contract export stays out of final runtimes.

## Diagnostic output

Follow the [logging policy](../architecture/runtime.md#diagnostic-logging) and [levels](../../crates/ipp-core/src/diagnostics.rs).

Native: `IPP_LOG=debug cargo run -p ipp-server --features websocket,diagnostics --locked`. Levels: `error`, `warn`, `info` (default), `debug`, `trace`, `off`.

Prepared browser render builds enable diagnostics: set `logLevel` in connection options or `IppCanvas.runtime`. Headless browser builds compile Rust logging out.

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
python tools/ipp.py build browser:headless browser:render-particles
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

The [Pages workflow](../../.github/workflows/gallery-pages.yml) builds, tests and uploads the site in one job and runs the retained GUI browser suite in a separate bounded, cached job; it deploys on pushes to `main` or manual runs from `main` only after both jobs pass, because the published gallery renders through the retained GUI path. Set the repository's **Settings → Pages → Build and deployment → Source** to **GitHub Actions** before the first deployment. Building locally does not publish the site.

Gallery builds require Blender 5.2 and Chromium (`python tools/ipp.py setup browser --with-deps`). Local development can install the pinned Blender archive with `python tools/ipp.py setup blender`; Pages CI installs the Blender Foundation Snap and verifies its 5.2 version. The build exports the maintained packed scenes into `target/gallery-platformer-assets/` and `target/gallery-gui-assets/projector/`; Platformer World serialization uses the matching worker/WASM contract. No KayKit archive download or projector texture rebake is needed. Runtime exports and authoring previews stay under ignored `target/`, while editable Blender/SVG sources and licenses remain in Git.

Gallery, Surface and GUI builds prepare [shared fonts](../../assets/fonts/README.md) through the `font-assets` prerequisite. Clean local and GitHub Actions builds download the pinned sources automatically and verify their SHA-256 checksums; subsequent builds reuse the verified `target/font-sources/` cache. `python tools/ipp.py build font-assets` prepares that cache and the shared runtime font directly.

The [catalog](../../tools/pipeline/catalog.py) owns products/checks, [profiles](../../tools/pipeline/profiles.json) own target selections, [suite groups](../../tools/pipeline/suites.json) select tests, and [test inputs](../../tools/pipeline/test-inputs.json) own each test's prerequisites. A test shared by multiple suites keeps the same declared inputs. Browser profiles build independently. Overlapping selections run each prerequisite and test once.

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

Every run writes a unique `target/pipeline/runs/run-*/summary.json`, incremental status and complete child logs. Build steps also write manifests with source-content identity, commands, tool/environment identity, dependency manifests, and output hashes. Reports describe the exact products observed in that run. Browser products retain separate build reports, and generation verifies the exported and final runtime contracts before publication.

The executor serializes writers within a checkout using an OS lock, released even after a crash. Use separate source worktrees for concurrent pipeline execution. It owns child processes, reports timeouts/cancellation, blocks dependents after failure and continues independent work unless `--fail-fast` is selected. Interactive development servers stream their URL and stay owned until stopped. A source change during execution is recorded explicitly; that run cannot establish one unchanged source snapshot.

Suites declare shared source ownership through `sourceRoots` in the [suite registry](../../tools/pipeline/suites.json): a root ending in `/` owns a directory, any other root one file. Changed-file selection includes every matching suite in addition to the conservative area rules; keep these roots aligned when moving shared implementation or test helpers. Files without a precise mapping still require an explicit suite selection.

Each invocation rebuilds declared prerequisites through the underlying incremental tools. There is no implicit result cache or build-skipping flag. New reports do not certify previous passing checks after edits. Retries read unfinished IDs, resolve the current catalog and never execute commands stored in a report:

```sh
python tools/ipp.py retry target/pipeline/runs/run-EXAMPLE/summary.json
python tools/ipp.py retry target/pipeline/runs/run-EXAMPLE/summary.json --suite cameras
```

Preserve applicable prior reports and reuse evidence only while its source, configuration and environment remain valid. Prerequisite reports and compile success do not prove runtime or rendering behavior. Maintained scenarios retain outcomes, frame observations and failure artifacts under `target/integration-artifacts/`.

### Regression entry point

Only an instructed merge into `main` or push triggers agent-run full regression. Working directly on `main` does not trigger it. One integration owner uses `python tools/ipp.py regression`, including retries and focused regression selections; `npm run regression` is the same entry point.

```sh
python tools/ipp.py regression --list
python tools/ipp.py regression --only check:repository --suite cameras
python tools/ipp.py regression --retry target/pipeline/runs/run-EXAMPLE/summary.json
LIBGL_ALWAYS_SOFTWARE=1 python tools/ipp.py regression --profile integration --egl-dir /usr/lib/x86_64-linux-gnu
```

The `repository` profile checks documentation, pipeline selection, Python formatting/types and executor tests. The `native` profile runs executor tests, workspace boundaries and native Clippy configurations on the current platform. The `integration` profile selects every maintained suite/check, all target distributions and native GLES scenarios. Focused selections report partial regression coverage. Required GLES coverage remains selected when libraries are missing: the prerequisite check fails instead of silently omitting it.

`IPP_EGL_LIBRARY_DIR` or `--egl-dir` selects actual EGL/GLES libraries. `NODE_BIN`, `BLENDER_BIN` and the invoking Python interpreter select executables; the executor passes the same selections to child harnesses. A Blender release installed through setup is resolved from `target/tools`. The browser inherits its configured environment; use the [Blender/browser environment guide](blender.md) on hosts with private library wrappers.

The [Pages workflow](../../.github/workflows/gallery-pages.yml) invokes the `gallery-site` and `retained-gui` browser suites through this CLI in separate jobs and uploads their evidence; the retained GUI job fails visibly when its browser environment is missing. Pipeline tests validate its commands against the current catalog. The repository, native and integration regression profiles remain available for local use; routine pushes do not run them as separate GitHub Actions jobs. Local validation does not claim that hosted jobs ran. Extend the maintained real native WebSocket, browser worker/WASM/WebGL and GLES scenarios when changing participating behavior; keep scenario intent separate from process setup.

## Release and size experiments

Build by package/target/explicit features; workspace unification can hide lean-build costs.

```sh
# Minimal native host library
cargo build -p ipp-server --release --no-default-features --locked

# Optional stable size profile; not an established distribution default
cargo build -p ipp-wasm --target wasm32-unknown-unknown --profile release-small --no-default-features --locked
cargo build -p ipp-wasm --target wasm32-unknown-unknown --profile release-small --no-default-features --features render --locked

# Inspect actual production dependencies and enabled features
cargo tree -p ipp-wasm --target wasm32-unknown-unknown --no-default-features --features render --edges normal,build --locked
```

`release-small` is defined in [Cargo.toml](../../Cargo.toml). Compare meaningful workloads against normal release, reporting raw/compressed WASM separately from JavaScript, shaders and native shims. Use `python tools/ipp.py measure <artifact> ...` for raw size, deterministic gzip size and SHA-256. Count shared files once and record toolchain/features; empty modules are not runtime size baselines.

Stable remains mandatory for normal builds. Add a pinned dated nightly only for concrete Miri/sanitizer verification or measured size experiments; no empty nightly jobs or unstable baseline dependency.

## Opt-in performance scenes

`python tools/ipp.py benchmark native|browser` runs the maintained [Blender stress scene](../../tests/performance/stress.md). Native runs accept `--egl-dir`; `--preset full` selects 10,000 animated cubes. Counters require the separate `--instrumented` native build. `python tools/ipp.py benchmark browser --scene retained-gui` compares analytic and retained Surface text over `--frames` streaming updates and rejects stress-scene options; the [retained rendering guide](../../tests/performance/retained-gui.md) describes its self-identifying reports and the physical-device procedure. Benchmarks and performance build products are excluded from regression profiles.
