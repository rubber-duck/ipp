# ECS and Serialization Strategy

[Runtime](../architecture/runtime.md) · [Protocol](../architecture/protocol-and-schema.md) · [Workspace](../architecture/rust-workspace.md)

## Storage and generated access

Extend stable typed storage, generational identities and metadata indexes together. Centralize invalidation before extending direct bindings; prove phase-scoped aliasing. Maintain dependency indexes from operation-local changes and reserve storage in its owning allocator. Private restoration installs typed values and references before selected Systems rebuild and validate derived state.

Use ordinary Rust types, derives and an explicit registry. Share validation and lifecycle handling across authored writes and structural animation; compile real-time numeric writes at binding boundaries under the runtime contract. Extend dynamic properties through the same paths, preserving property-level invalidation and sparse restoration. Measure mutation cost on representative Worlds before adding caches or alternate storage.

Component ownership lives in [`ipp_core::components`](../../crates/ipp-core/src/components/mod.rs): `schema`, `registry`, and `dynamic_properties` are the canonical paths, alongside lifecycle, primitives, and storage; crate-root re-exports cover value types only. World-side component-state behavior lives in [`world/component_state`](../../crates/ipp-core/src/world/component_state/mod.rs), split by phase (`access`, `staging`, `mutation`, `observations`), with `component_binding` and `component_query` as private world-root helpers.

## Target contracts and generation

Extend [executed-target export](../../tools/ipp-schema-gen/README.md), keeping schema work outside evaluation. Verify repeatable generation and capability omission, then connect matching generated clients to real Hosts. Compiler fixtures cover field access; executed targets prove layout.

## World persistence

Extend the [serialization service](../../crates/ipp-core/src/services/world_serialization) through per-system capture/restore hooks and shared identity mappings under the [snapshot contract](../architecture/protocol-and-schema.md#snapshots-and-world-replacement). Keep codecs and format details with implementation.

Exercise underlying-value recovery, excluded ownership, controller restoration and pending resources together. Restore identities before references and bindings; apply capacity hints before allocation. Use ordinary authored base state for durable React or Blender content.

Extend generic readers/writers and Host transfer state together. Test partial progress, cancellation and complete-only publication. Measure capture, encoding, restoration and transfer memory separately; synchronous work can pause the Host. Add compression, indexing or cooperative execution only for measured benefit and within accepted design. [Asset bundling](../architecture/assets.md#asset-output-and-durable-bundles) remains separate from ordinary reference-only saves.

## GUI state

Extend component-owned node/part storage and sparse property bindings under the [GUI identity contract](../architecture/gui.md#identity-and-authoritative-state). Share validation across incremental authoring, runtime control actions and generated value operations; fence stale root/node lifetimes before reusing storage. Generate native and executed-WASM contracts together, keeping computed geometry internal and GUI-disabled builds free of its registrations.

Use the existing per-System snapshot hooks to capture eligible structure, asset references and committed values while excluding GUI interaction state and only its transient playback. Reconstruct layout and resolved skin state after restoration; preserve ordinary animation persistence. Extend real native/worker lifecycle, contract and snapshot scenarios with independent expected values, fresh-handle rejection and completed restored-frame captures. Include saving during provisional composition and queued input so durable output cannot accidentally capture transient edits. Keep fixtures and assertions independent of transport/process arrangement through the maintained drivers.

## Integration harness

Extend the maintained `snapshots`, `lifecycle`, `contracts` and `scaling` [suites](../../tools/pipeline/suites.json). Use generated clients through native WebSocket and browser worker/WASM environments, with local immutable resources and independently authored World/controller fixtures.

| Evidence | Observable result |
| --- | --- |
| Round trip | Underlying state, durable identities and controller position survive; runtime handles are fresh |
| Isolation | Excluded ownership stays excluded; rejected loads preserve published Worlds |
| I/O | Save performs no asset reads; cancellation, unavailable sources and ordered transfers remain observable |
| Rendering | Restored ready resources produce matching completed frames |
| Scale | Maintained mutation/restoration fixtures retain operation counts and release timings, including synchronous pauses |

Keep assertions independent of process and wire setup so new transports reuse scenarios. Environment drivers own readiness, gated sources, frame capture and cleanup under the [testing policy](../development/integration-testing.md). Codec and memory-safety tests supplement this real integration evidence; detailed cases live with the tests.
