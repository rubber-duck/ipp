# Protocol, Schema and World Replacement

[Architecture overview](../architecture.md) · [Implementation strategy](../plans/ecs-serialization.md)

## Control and data planes

Control carries typed operations, identities, subscriptions and references; bulk payloads use the [owned data plane](assets.md#data-plane-ownership). Transports enqueue ordered batches for core validation, either complete or as bounded buffers under a Host-issued logical batch identity fenced to the World session. Backpressure or explicit rejection must preserve batches, outcomes and ownership transfers without silent coalescing, dropping or splitting across frames. Provisional aliases are connection/batch-scoped and resolve before retention. Client writers page logical edits automatically and may pipeline bounded in-flight buffers, aggregating outcomes for the application. Chained buffers acknowledge applied identities independently of a completed frame. Consumed page storage is recyclable while the logical batch remains open. Only an explicit terminator completes the logical batch; a shorter buffer may result from other framing limits and never implies completion. The Host rejects completed, expired and foreign batch identities; completion and the bounded Host-time deadline follow the [mutation boundary](runtime.md#mutation-and-evaluation-boundaries).

Account ingress admission and reliable-output delivery separately per connection. Hosts serve connections fairly, reserve reply capacity before accepting correlated work and throttle congestion with hysteresis. Exhausted output capacity or sustained delivery stalls disconnect only the affected connection with an explicit reason; healthy peers/Worlds continue.

## World-scoped connections

```mermaid
flowchart LR
    connect["Connect / verify contract"] --> unattached["Host connection"]
    unattached -->|"Create / load / attach"| session["World session"]
    session -->|"Detach"| unattached
```

Host discovery, creation/loading, naming and destruction are separate from World authoring. Name collisions never replace Worlds; failed operations leave the connection usable. Creation/loading/attachment require an unattached connection. Each logical session selects one World, initially one attachment per connection; physical multiplexing cannot change that scope.

Commands, queries, replies and events validate World identity/incarnation and session identity. Stale work cannot cross attachments. World-local entity/declaration/subscription handles are distinct from Host asset identities.

Worlds normally outlive connections and keep updating without clients. Destruction is explicit/Host policy, with optional temporary-World destruction on owner disconnect. Disconnect releases session ownership, subscriptions and producer registrations. Shared attachments receive correlated replies/relevant events while their World updates once per Host frame. [Runtime ownership](runtime.md#host-services-and-worlds) governs services, clocks and surfaces.

## Operations and outcomes

Batch outcomes distinguish completion, operation failure and commit failure, retaining applied identities for cleanup/correction under [partial mutation](runtime.md#mutation-and-evaluation-boundaries). Commit failures name an originating operation only when known. Resource, playback and ownership events remain separate.

Session readiness establishes authoritative metadata/component manifests; it does not establish resource readiness or a completed frame. Inspection uses targeted reads and bounded pages with evaluated ticks. Pages are independent observations; oversized individual records fail explicitly without truncating state or closing the connection.

## System commands, queries and events

| Kind | Contract |
| --- | --- |
| System command | No correlated reply; invalid commands report diagnostics and may retain partial effects |
| Query | One correlated result/failure while the session lives |
| State-change event | Independent sparse observation of committed state |

Commands/queries use ordered ingress without advancing time. Events preserve applied transitions even after batch failure or later changes; no-ops emit nothing. Encoding/transport failures are separately observable, and World batches retain their acknowledgement/ownership outcomes.

## Generated field access

An explicit compiled registry defines deterministic identities. Access validates exact exposed fields, types and supported operations; offsets, padding and owned-value internals are never arbitrary client targets. Creation requires a compiled contract independently of binding existing instances. [Internal fields and storage](runtime.md#state-and-identity) remain local implementation state.

Compiled components may opt into named typed dynamic properties without runtime type registration. Properties belong to components independently of shaders or other consumers, use validated identities, and participate in generated writes, overlays, animation and persistence. Asset-valued properties carry general typed source/variant references; a shader's texture requirements do not restrict that model. [Runtime binding rules](runtime.md#stable-storage-and-direct-bindings) own invalidation and relocation.

Native writes, protocol and overlays share validation/lifecycle rules. Real-time numeric evaluation uses prevalidated bindings and derived-result notification under the [runtime binding contract](runtime.md#stable-storage-and-direct-bindings). Retained values own storage; transient decoding views cannot escape. Schema processing stays outside evaluation hot paths.

[GUI](gui.md#identity-and-authoritative-state) extends these contracts with root/node lifetime checks and revision-aware value operations. Ordered input admission, routed observations and committed control effects remain distinct. Its snapshot exclusions are scoped to GUI transient interaction state under the [GUI persistence boundary](gui.md#client-and-persistence-boundaries).

## Build compatibility

Export and generate Host/SDK contracts from the actual target with the final build's capability selection. The compatibility hash covers layout, defaults, operations, events and encoding, rejecting mismatches before normal decoding. Host layouts never substitute for WASM. CPU layout, wire encoding and GPU packing remain distinct; codecs share a target-independent transport/session interface. [Generation tooling](../../tools/ipp-schema-gen/README.md) owns export mechanics.

Before stabilization there is no portable ABI or backward-compatibility promise. Change contracts, assets, SDKs and saved Worlds together; regenerate clients/fixtures rather than adding legacy formats, adapters or migrations unless explicitly requested.

## Snapshots and world replacement

Snapshots retain metadata/hints, producer components, controller state and driver descriptions, reconstructing underlying values through sparse restoration. Active animation transitions retain their semantic clocks and sparse origins under the [animation contract](runtime.md#animation-and-constraints). Asset references remain unchanged; saving never acquires or embeds bytes. Exclude reconstructible sampled caches, other internal data, pointers, GPU state, overlays, Auto fallbacks and owner-scoped entities/components; references to excluded identities reject export.

Systems own durable capture/restore hooks; World/serialization own enumeration, framing and publication. Install entities/components before rebuilding bindings in dependency order. Load validates a private compatible World before publishing and attaching a fresh session. Failure preserves published Worlds; detach fences queued work/releases session ownership without destroying the previous World. Clients recreate desired declarations with fresh handles.

Destructive replacement, cross-schema migration, merging, overlay baking and incremental reconnect recovery remain deferred. [Asset bundling](assets.md#asset-output-and-durable-bundles) is a separate operation.

### Durable world identity and save boundaries

Durable World/entity identities are assigned once and survive load independently of runtime handles, editable names and allocation order. They imply neither merging nor reconnect recovery.

Save captures owned state at an ordered mutation boundary. Later edits cannot change the capture; output holds no World borrow. Detach, destruction and cancellation invalidate unpublished originating-session work. Publish only complete output; bounded temporary storage may fail explicitly.

Capture, encoding and private restoration are currently synchronous and can pause other Worlds. Hosts admit expensive work fairly between frames, account retained transfers separately from active scratch and reclaim inactive transfers. Bounded transfer progress does not imply cooperative persistence.

Restore controller clocks/status and semantic bindings; sample saved position after resource readiness before advancing. Reserve storage from defaults → saved hints → explicit overrides, increasing for serialized counts as needed. Save configured hints, not allocator capacities; presentation selections remain excluded.

Implementation: [serialization](../../crates/ipp-core/src/services/world_serialization), [protocol](../../crates/ipp-protocol/src), [client](../../packages/ipp-client/README.md).
