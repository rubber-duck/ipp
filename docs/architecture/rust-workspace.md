# Rust Workspace and Build Composition

[Architecture overview](../architecture.md) · [Build guide](../development/building.md) · [Implementation strategy](../plans/runtime-and-rendering.md#build-composition-and-size)

## Crate boundaries

Crates enforce ownership/platform isolation. Related systems stay modules; shared crates need real consumers.

| Crate | Owns |
| --- | --- |
| `ipp-core` | Headless state, lifecycle, evaluation and logical assets |
| `ipp-protocol` | Bootstrap, generated codecs and snapshot encoding |
| `ipp-host-session` | Shared queues, frame dispatch, correlation and responses |
| `ipp-render-gl` | WebGL/GLES rendering and GPU resources |
| `ipp-wasm`, `ipp-server` | Platform clocks, transport, I/O, contexts and presentation |
| `ipp-schema-derive`, `ipp-schema-gen` | Compile-time access and verified-target generation |

Hosts depend on session, protocol, optional renderer and core; session depends on protocol/core; protocol and renderer depend on core. Core never depends on hosts, codecs or renderers. Shared session policy excludes sockets, workers and graphics contexts. Rendering embeds without networking; Blender ships separately.

## Compile-time composition

The headless baseline includes spatial state, property animation, constraints, overlays, persistence, material/texture/light declarations and geometry queries. Rendering includes unlit/PBR, textures and geometry visualization. Skeletal animation, mesh poses, particles, surfaces, shadows and built-ins are optional scene capabilities. Surface conversion dependencies stay in offline tooling; disabling surfaces omits their types, registrations and shaders.

The optional [GUI capability](gui.md) depends on surfaces. Its layout and interaction remain headless; browser input services stay in client adapters. GUI-specific registrations, text-editing dependencies and rendering extensions are omitted when disabled, while ordinary Surface labels remain available. Target contracts advertise implemented operations only.

One skeletal selection covers pose evaluation, joint animation, skin bindings/palettes and deformation through separate ordered systems. Runtime demand selects work/resources within compiled capabilities. Disabled capabilities omit code, registrations, dispatch, assets and shaders; do not add placeholder flags.

Core/host defaults include built-ins; renderer/build-tool defaults are empty. Minimal builds retain baseline behavior. Rendering, WebSocket, ZIP, diagnostics and contract export stay at their owning boundaries. [Manifests/configurations](../development/building.md#crates-and-features) own forwarding details.

## Compile-time generation

Follow [target compatibility](protocol-and-schema.md#build-compatibility): explicit registry membership/identities, target-compiled derives and executed-target export. Discovery/macro/linker order never assigns identity; host macro execution never determines target layout. Diagnostics may compile out without changing schema identity. Compiler parsers, export/test tooling and native context shims stay outside runtime/browser dependencies as applicable.

## Third-party dependency policy

Use pinned stable Rust, reserving dated nightly for specific verification or measured experiments. Justify production dependencies by purpose, enabled/transitive features, maintenance and artifact cost. Prefer std/small glue when sufficient; established libraries when correctness/interoperability warrants them.

Own networking in hosts, graphics in renderers and conversion outside viewers. Keep runtime decoders optional, versions central and lockfiles retained. Build release artifacts by package/target/capability to avoid hidden workspace feature unification. Measure WASM, JavaScript, shaders and native shims separately; validate representative minimal/expanded builds through real integration. All-features compilation proves neither omission nor delivery.

## Development tooling

Python owns development command planning, prerequisite discovery, process supervision and artifact evidence. Node performs JavaScript bundling and target WASM execution; Cargo and Rust tools own compilation and schema generation. All public build, test and example commands use the same declared products and prerequisites. Generated clients remain paired with their actual target runtime. Tooling stays outside runtime dependencies; Beads retains work tracking. The [build guide](../development/building.md) owns setup and command usage.
