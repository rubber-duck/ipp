# ECS and Serialization Strategy

[Runtime](../architecture/runtime.md) · [Protocol](../architecture/protocol-and-schema.md) · [Workspace](../architecture/rust-workspace.md)

## Storage and generated access

Extend stable typed storage, generational identities and metadata indexes with the [core ordered link store](../architecture/runtime.md#object-hierarchy). Keep one stored relationship value and derive adjacency and traversal indexes from changed links. Exercise edits, structural animation and its restore on stop, deleted-generation references, joint propagation and cycle correction through the same lifecycle. Private restoration installs typed values and references before selected Systems rebuild indexes and validate derived state.

Allocate component pages and substantial System reservations only for actual demand. Preflight required allocations before destructive mutation, preserve occupied addresses and notify prepared queries/bindings when missing storage becomes present. Compare many small selected-system Worlds with representative populated Worlds; a World selecting only Canvas systems must not pay for every compiled component type.

Use ordinary Rust types, derives and an explicit registry. Share validation and lifecycle handling across authored writes and structural animation; compile real-time numeric writes at binding boundaries under the runtime contract. Retain dynamic properties and compact schema rows through the same property-level invalidation, staging and persistence paths. Rows remain appropriate for theme parts and joint overrides; ordinary GUI entities no longer require parallel node tables. Measure mutation cost on representative Worlds before adding caches or alternate storage.

Implement compare-and-set as a generic field operation and field subscriptions as an extension of the lifecycle watch, sharing its membership, per-connection output accounting, Host drain and client code rather than adding a sibling facility. Compare observed fields after every System's finish pass so observations see final stored values, and keep unchanged fields free of allocation. Measure observer cost with thousands of watched controls.

Component ownership lives in [`ipp_core::components`](../../crates/ipp-core/src/components/mod.rs): `schema`, `registry`, and `dynamic_properties` are the canonical paths, alongside lifecycle, primitives, and storage; crate-root re-exports cover value types only. World-side component-state behavior lives in [`world/component_state`](../../crates/ipp-core/src/world/component_state/mod.rs), split by phase (`access`, `staging`, `mutation`, `observations`), with `component_binding` and `component_query` in the World's private [`world/direct_bindings`](../../crates/ipp-core/src/world/direct_bindings/mod.rs) module.

## Target contracts and generation

Extend [executed-target export](../../tools/ipp-schema-gen/README.md) for core links, typed World/output references and selected-system manifests, keeping schema work outside evaluation. Distinguish the build's available schema from each World's admitted components and operations, and enforce both in core and protocol. Regenerate matching clients and fixtures directly, removing the GUI structural-command lane. Verify repeatable generation, capability omission and typed row access through real native and executed-WASM targets.

## World persistence

Extend the [serialization service](../../crates/ipp-core/src/services/world_serialization) to a coherent Host-owned graph cut under the [snapshot contract](../architecture/protocol-and-schema.md#snapshots-and-world-replacement). Use graph-local World identity to distinguish copies with equal durable IDs, preserving durable metadata while remapping runtime handles and references. Capture authored attachments and nested OutputRefs; omit Host root presentation bindings. Keep codecs and format details with implementation.

Exercise stored-value round trips, applied animation contributions, selected Systems, controller restoration and pending resources together. Restore all graph identities before references and bindings; restore/remap controller bindings for clip-local entity references without rewriting immutable asset key slots per World or load. Validate before publishing any World. Keep System selection separate from capacity hints. React and Blender content persists as ordinary stored state; test sibling copies sharing durable IDs without merging.

Extend native/worker snapshot scenarios with [data-source binding state](../architecture/data.md#persistence). Verify that configuration, column-binding asset references, parameter properties, windows and source names survive without runtime handles, source samples or evaluated views, and that restored bindings report unavailable data until a client creates a compatible source incarnation. Saving binding state must not introduce dataset payload export.

Extend generic readers/writers and Host transfer state together. Test partial progress, cancellation and complete-only publication. Measure capture, encoding, restoration and transfer memory separately; synchronous work can pause the Host. Add compression, indexing or cooperative execution only for measured benefit and within accepted design. [Asset bundling](../architecture/assets.md#asset-output-and-durable-bundles) remains separate from ordinary reference-only saves.

## GUI state

Keep GUI structure, configuration, style, raw Canvas leaves and control state in ordinary entities and component fields under the [GUI identity contract](../architecture/gui.md#identity-and-authoritative-state). Retain compact theme/part tables and incremental layout/paint algorithms. Layout, measurement and Canvas read fields directly without building owned snapshots or walking ancestors; a value change should dirty only its control's paint, a text change only its remeasure and a scroll change only its scope. Validate local changes proportionally and preserve entity/component fences in both native and executed-WASM contracts.

Ordinary entity persistence saves control values with structure, configuration, themes and assets; GUI needs no persistence hook for its entities, and its presentation preferences persist as World-level System state. Reconstruct layout and skin output and preserve ordinary animation persistence. Extend real native/worker snapshots with provisional composition, queued input and adoption of restored controls by a reconnecting client. Keep independent expected values and completed restored-frame assertions through maintained drivers.

## Integration harness

Extend the maintained `snapshots`, `lifecycle`, `contracts` and `scaling` [suites](../../tools/pipeline/suites.json). Use generated clients through native WebSocket and browser worker/WASM environments, with local immutable resources and independently authored World/controller fixtures.

| Evidence | Observable result |
| --- | --- |
| Round trip | Stored state, applied animation contributions, durable identities and controller position survive; runtime handles are fresh |
| Isolation | Rejected graphs publish no Worlds; equal durable IDs do not merge copies |
| I/O | One coherent graph cut; cancellation and unavailable sources remain observable; save performs no asset reads |
| Rendering | Restored ready resources produce matching completed frames |
| Scale | Maintained mutation/restoration fixtures retain operation counts and release timings, including synchronous pauses |

Keep assertions independent of process and wire setup so new transports reuse scenarios. Environment drivers own readiness, gated sources, frame capture and cleanup under the [testing policy](../development/integration-testing.md). Codec and memory-safety tests supplement this real integration evidence; detailed cases live with the tests.
