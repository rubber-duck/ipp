# Rust Workspace and Build Composition

[Architecture overview](../architecture.md) · [Build guide](../development/building.md) · [Implementation strategy](../plans/runtime-and-rendering.md#build-composition-and-size)

## Crate boundaries

Crates enforce ownership/platform isolation. Related systems stay modules; shared crates need real consumers.

| Crate | Owns |
| --- | --- |
| `ipp-core` | Headless state, lifecycle, evaluation and logical assets |
| `ipp-protocol` | Hello and contract exchange, generated codecs and snapshot encoding |
| `ipp-host-session` | Host task execution, shared queues, frame dispatch, correlation and responses |
| `ipp-render-gl` | WebGL/GLES rendering and GPU resources |
| `ipp-wasm`, `ipp-server` | Platform clocks, transport, I/O, contexts and presentation |
| `ipp-schema-derive`, `ipp-schema-gen` | Compile-time access and verified-target generation |

Hosts depend on session, protocol, optional renderer and core; session depends on protocol/core; protocol and renderer depend on core. Core never depends on hosts, codecs or renderers. Shared session policy excludes sockets, workers and graphics contexts. Rendering embeds without networking; Blender ships separately.

## Compile-time composition

A build varies along three axes and nothing else:

| Axis | Choices | Decides |
| --- | --- | --- |
| Instrumentation | Production, instrumentation | Whether test controls and profiling hooks are compiled |
| Rendering backend | None, WebGL 2, GLES 3 | The one renderer a host distribution links; never two, never an unused one |
| Host target | Native, WASM | Platform clocks, transport, I/O and contexts |

Every scene capability is compiled into every build: spatial state, property and skeletal animation, constraints, persistence, mesh poses, particles, Surfaces, GUI, shadows, built-ins, and material/texture/light declarations with geometry queries. A rendering build carries each capability's presentation path for its one backend. A capability is not a Cargo feature, and adding one does not add a build configuration.

A World excludes capabilities at runtime. Each World chooses an immutable dependency-checked System selection under the [runtime composition contract](runtime.md#system-composition), with a manifest for the operations it can evaluate. Selection does not register runtime types or change compiled field layouts. A compiled capability that no World selects costs code size only: its Systems, component pages and renderer resources are created on demand. One skeletal selection covers pose evaluation, joint animation, skin bindings/palettes and deformation through separate ordered systems.

Each host target has one compiled schema and one target contract. Instrumentation and the rendering backend never change schema identity, so a client generated for a target works with every build of that target.

Cargo features exist only for the instrumentation axis (`instrumentation`), the WASM host's renderer (`render`) and core's test-only invariant oracle (`checked-invariants`). Every build logs, answers statistics requests and installs a panic hook that reports through its log sink, so a production fault is diagnosable; [diagnostic logging](runtime.md#diagnostic-logging) keeps that cost off hot paths. Every build carries its contract descriptors and can [serve them to a client](protocol-and-schema.md#build-compatibility). Platform dependencies follow the host target: the native server always includes its WebSocket transport. Do not add capability or placeholder flags. [GUI](gui.md) layout and interaction remain headless, browser input services stay in client adapters, and Surface conversion dependencies stay in offline tooling. [Manifests/configurations](../development/building.md#crates-and-features) own the declarations.

Measure artifact size per host distribution without default size ceilings. Keep it down through shared implementation and demand-driven resources; size measurements never select capabilities at compile time. Runtime resource isolation budgets, when explicitly configured for a World or attached child World, are a separate policy from distribution size and require their own resource accounting and enforcement design.

## Compile-time generation

Follow [target compatibility](protocol-and-schema.md#build-compatibility): explicit registry membership/identities, target-compiled derives and executed-target export. Discovery/macro/linker order never assigns identity; host macro execution never determines target layout. Instrumentation never changes schema identity. Compiler parsers, generation/test tooling and native context shims stay outside runtime/browser dependencies as applicable.

## Third-party dependency policy

Use pinned stable Rust, reserving dated nightly for specific verification or measured experiments. Justify production dependencies by purpose, enabled/transitive features, maintenance and artifact cost. Prefer std/small glue when sufficient; established libraries when correctness/interoperability warrants them.

Own networking in hosts, graphics in renderers and conversion outside viewers. Keep a runtime decoder only in the host targets that use it, versions central and lockfiles retained. Build release artifacts by package, target and axis selection to avoid hidden workspace feature unification. Measure WASM, JavaScript, shaders and native shims separately, reporting attribution and unknown costs without default size ceilings; validate the production and instrumentation builds of each host distribution through real integration, with scenarios on the production build unless they need a test control or the profiler. Compilation alone does not prove delivery.

Native Hosts use Smol-compatible task and I/O libraries; browser Hosts retain their platform I/O bridges and connect completion to Rust futures. Select portable Smol task/executor components for Host scheduling without linking the native runtime/reactor into browser WASM. Core's asynchronous I/O contracts remain executor-independent. Validate actual target support and measure transitive and artifact costs before selecting concrete adapters; a shared async API does not imply a shared operating-system backend.

## Development tooling

Python owns development command planning, prerequisite discovery, process supervision and artifact evidence. Node performs JavaScript bundling and target WASM execution; Cargo and Rust tools own compilation and schema generation. All public build, test and example commands use the same declared products and prerequisites. Generated clients remain paired with their actual target runtime. Tooling stays outside runtime dependencies; Beads retains work tracking. The [build guide](../development/building.md) owns setup and command usage.
