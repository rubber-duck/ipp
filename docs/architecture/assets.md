# Assets and Resources

[Architecture overview](../architecture.md) · [Implementation strategy](../plans/runtime-and-rendering.md#asset-transport)

## Stable resources and resource-owned loading

Resources identify immutable named content; changed content requires a new name. Providers own loading, readiness, cancellation and recovery; encoding versions express compatibility, not content history. Core owns identity/lifetime, consuming subsystems own CPU asset types, and the renderer owns graphics representations.

The Host's AssetManagementService directly owns catalog, providers and acquisition without a parallel loading state machine. World systems own dependencies. Generational keys validate identity and availability; copying a key does not retain it. Explicit consumer accounting, including hidden base references, preserves identity across unload/reload; reuse changes generation. Evaluation borrows payloads. Resources may expose multiple variants, with visibility/quality requesting residency independently of retention.

## Asset lifecycle propagation

The Host forwards asset changes through every World's [System lifecycle boundary](runtime.md#lifecycle-and-state-access). Consumers own invalidation/recovery; the lifecycle publisher owns subscriptions, never loading. Finish all invalidation before release/reuse; asynchronous client delivery cannot hold that barrier. Update-requested destructive releases wait for all World updates and temporary service borrows to end.

## Assets shared between worlds

Aggregate demand per World/system consumer. Mutation changes only that consumer's demand, without interleaved service progress; applied demand survives batch failure. One World's teardown cannot cancel another's usage.

Shared identities include type, variant and source namespace. Matching IDs/URLs alone do not make content interchangeable or erase producer authorization/scope. Explicitly shareable immutable sources may deduplicate. Content can outlive producers, but lost recovery sources must fail rather than retarget replacement producers.

There are no asset-count, input or queued-byte admission quotas. Memory, representable identities and actual device capabilities bound growth; allocation failure remains observable. Hosts own a configurable soft memory budget for caching unused loaded resources. Required consumer data and producer ownership remain authoritative even when they exceed the cache budget.

## Source references and resource providers

DataSourceManagementService independently supplies listing and capability-checked readers/writers, allowing serialization I/O without asset objects. Core needs no networking library or executor. Source I/O, typed decoding and GPU residency have distinct owners.

Register opaque, non-overlapping string prefixes; reject duplicates and overlap in either direction. Route literally and forward the complete identifier unchanged. Sources validate syntax; generic routing never parses, normalizes or decodes identifiers.

Requests and registrations carry producer/resource/session scope. Cancellation and replacement fence stale completions; references contain no process pointers. Adapters never reenter Worlds or call graphics APIs. [Data-source implementation](../../crates/ipp-core/src/services/data_source) owns buffering and adapters. Optional [built-ins](../../crates/ipp-core/src/services/asset_management/builtin) use the same loading/recovery path without special World or renderer behavior.

## Shader recipes and graphics loading

Immutable shader recipes specify compilation features and required passes; component values never create program variants. Renderer-owned providers compile/link programs and upload meshes before graphics readiness. Built-in/custom programs share this lifecycle; headless Hosts may retain authored recipes without claiming GPU readiness.

Hosts register narrow device/compiler dependencies and progress loaders with the correct context outside World evaluation. Loaders never borrow Worlds or locate services through World state. Retain only handles/layout metadata and CPU data required by actual consumers; release temporary decode/compile inputs. [Graphics loaders](../../crates/ipp-render-gl/src/services/render) own representation details.

## Client-authored sources

Clients register immutable names in session-isolated scene scopes; scene-local IDs are authoring bindings, not mutable resource aliases. Retain a recovery source while producer ownership or consumers require it. Explicit preparation loads before component selection, allowing editors to keep prior selections until replacements are ready. Release removes only that producer's ownership/demand. Registration, decoded data and GPU allocation have independent lifetimes.

## Geometry definitions

Authored/generated immutable [bounding and picking definitions](rendering.md#bounding-and-picking-geometry) belong to assets; instances own evaluated shapes. Skeletal definitions depend on mesh, skin binding and skeleton together. Fitting belongs with implementation.

## Surface resources

The optional Surface capability consumes immutable font and drawing assets with shared quadratic contour data. Fonts preserve glyph identities and headless layout metrics; drawings preserve ordered painted paths and fill rules. Conversion owns source-format interpretation and approximation, while the renderer owns acceleration structures, curve textures and device-specific packing. Curve textures keep contour coordinates exact, using the narrowest [fixed-point texel format](../../crates/ipp-render-gl/src/services/render/surface_path.rs) that represents them. Bitmap resources preserve colour and coverage alpha independently, with explicit colour conversion under the rendering contract.

[GUI text and skins](gui.md#text-and-skins) consume these same resources and ordinary animation clips. GUI adds no separate loading or asset-identity system; font measurement remains headless, and skin values stay independent of immutable resource content.

## Pending resources and rendering

Producers may declare immutable source identities before producing bytes. The source provider owns the pending index and availability notifications; pending readers wait without occupying active transfer slots. Ready and failed notifications release those waits, while decoded and graphics readiness remain separate asset lifecycle states. Producer disconnection fails unresolved reads instead of silently retargeting them.

Valid references commit independently of readiness; I/O/failure never blocks or rolls back them. Partial/invalid payloads are unavailable. Missing required data skips affected work while ready items continue; later readiness needs no resubmission. Preparation before selection does not make batches atomic. GPU failure cannot publish partially usable graphics data and preserves usable CPU geometry and sessions.

## Data-plane ownership

Asset payloads travel exclusively through the data plane, including client-authored immutable sources. World commands carry references and never embed asset bytes. Source delivery and its acknowledgements progress independently of World command batches; registration does not imply decoded or graphics readiness. Bulk transfers hand exclusive ownership to the Host. Retained decoded data cannot borrow transient input. Shared memory requires synchronized immutable access and explicit release before reuse; cross-process descriptors imply no usable pointers.

Backpressure bounds chunks, not total asset size. Use checked lengths and fallible allocation where supported. Streaming does not promise incremental decoding or zero-copy; complete buffering may be necessary.

## Retention and recovery

Identity, CPU availability and GPU residency are independent. Unload clears payload availability while retained references preserve identity; [invalidation](#asset-lifecycle-propagation) precedes release. Recovery must reproduce the same immutable content from retained data or a reliable source, otherwise fail explicitly.

Losing the last consumer does not immediately discard successfully loaded immutable content with a valid external recovery source. Retain it while the unused-resource cache has space, and reclaim idle entries under pressure using both their measured size and recency of use. Returning demand can reuse cached content without another acquisition. Cache ownership never grants a World access to another producer's private sources, retains abandoned transfers, or overrides explicit unload, source revocation and session cleanup. Eviction uses the ordinary synchronous invalidation and generation barriers; CPU and graphics allocations remain separately accounted.

Context loss preserves logical state while invalidating GPU work/allocations. Providers reconstruct graphics representations while preserving usable decoded data/metadata even after failed recovery. CPU consumers test decoded availability independently of graphics readiness; account CPU retention and GPU allocations separately. [Rendering](rendering.md#residency-and-recovery) owns context execution/failure scope.

Browser source delivery and Host-owned decode/graphics loading progress independently of simulation, with concurrent readers and bounded chunks. The Host keeps graphics progress behind its live-context barrier, while World-visible outcomes and events remain frame publications. Inactivity excludes intentional backpressure. Native filesystem access requires an explicit root/prefix and is disabled by default.

## Asset output and durable bundles

Core provides executor-independent asynchronous output and typed encoding from retained CPU data or validated encoded content. Hosts own destinations/publication. Output backpressure, cancellation and errors stay separate from loading and World outcomes. Publish only complete output, preserving existing destinations on failure where writers promise atomic publication.

[World saves](protocol-and-schema.md#snapshots-and-world-replacement) preserve references without acquiring, packing or rewriting assets. Bundling remains a deferred operation that relocates assets and updates references before ordinary serialization; there is no asset-gathering save job.
