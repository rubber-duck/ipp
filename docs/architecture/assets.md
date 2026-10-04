# Assets and Resources

[Architecture overview](../architecture.md) · [Implementation strategy](../plans/runtime-and-rendering.md#asset-transport)

## Stable resources and resource-owned loading

Resources identify immutable named content; changed content requires a new name. Providers own loading, readiness, cancellation and recovery; encoding versions express compatibility, not content history. Core owns identity/lifetime, consuming subsystems own CPU asset types, and the renderer owns graphics representations.

The Host's AssetManagementService directly owns catalog, providers and acquisition without a parallel loading state machine. World systems own dependencies. Generational keys validate identity and availability; copying a key does not retain it. Explicit consumer accounting preserves identity across unload/reload; reuse changes generation. Evaluation borrows payloads. Resources may expose multiple variants, with visibility/quality requesting residency independently of retention.

## Asset lifecycle propagation

The Host forwards asset changes through every World's [System lifecycle boundary](runtime.md#lifecycle-and-state-access) and retained completed publications, including frozen branches. Consumers own invalidation/recovery; the lifecycle publisher owns subscriptions, never loading. Finish all invalidation before release/reuse; asynchronous client delivery cannot hold that barrier. Update-requested destructive releases wait for all World updates and temporary service borrows to end.

## Assets shared between worlds

Aggregate demand per World/System and completed-publication consumer. Mutation changes only that consumer's demand, without interleaved service progress; applied demand survives batch failure. Retained output remains a live consumer even while its World does not evaluate. One World's teardown cannot cancel another's usage; detachment and publication retirement release only their own demand.

Shared identities include type, variant and source namespace. Matching IDs/URLs alone do not make content interchangeable or erase producer authorization/scope. Explicitly shareable immutable sources may deduplicate. Content can outlive producers, but lost recovery sources must fail rather than retarget replacement producers.

There are no asset-count, input or queued-byte admission quotas. Memory, representable identities and actual device capabilities bound growth; allocation failure remains observable. Hosts own a configurable soft memory budget for caching unused loaded resources. Required consumer data and producer ownership remain authoritative even when they exceed the cache budget.

## Source references and resource providers

IoService owns generic listing, capability-checked byte readers/writers, source routing, transfers, cancellation and backpressure, including persistence I/O without asset objects. Asset Management and the [Data Service](data.md#service-and-source-ownership) both build on it; it knows neither dataset rows nor retention windows. Core needs no networking library or executor. Byte I/O, typed decoding and GPU residency have distinct owners.

Opening sources, listing entries, reading and writing are asynchronous operations scheduled through the Host's [task execution contexts](runtime.md#task-scheduling). Readers expose scoped immutable byte windows at a logical cursor through an awaitable read. A consumer requests a minimum contiguous length; a ready window contains at least that many bytes and may expose more, while final input may return a shorter window with explicit EOF. Insufficient input remains pending without advancing the cursor. Consumption advances only by the prefix the consumer accepts; releasing a window or cancelling an individual pending read alone does not consume it. Dropping the reader cancels acquisition. Readiness never requires a World frame or advances simulation time.

The same reader contract serves shared buffers and streams, including dynamically registered providers without requiring a heap allocation for each read future. Directly addressable immutable backing, including mapped files and shared memory, lends existing ranges without copying or conversion into an owned vector. Stream readers fill reusable reader-owned storage and coalesce only when the requested contiguous span requires it. Buffering follows consumer lookahead and bounded prefetch, independently of transport chunk boundaries; it does not assemble the whole source by default. A requested span must be fulfilled, reach final input or fail explicitly, never wait forever behind a smaller fixed transport buffer. Consumers decode from these windows into private destination storage. I/O owns backing lifetimes and buffering, and Hosts own platform adapters, keeping core independent of operating systems and JavaScript.

Open operations capture an exact source registration; replacement cannot retarget outstanding handles. Listings yield entries incrementally. Writers apply backpressure and report accepted prefixes, with separate flush and completion operations; cancellation cannot undo accepted output. Completion preserves a destination's promised publication guarantees. WebSockets remain message transports rather than byte-source readers.

Register opaque, non-overlapping string prefixes; reject duplicates and overlap in either direction. Route literally and forward the complete identifier unchanged. Sources validate syntax; generic routing never parses, normalizes or decodes identifiers.

Requests and registrations carry producer/resource/session scope. Cancellation and replacement fence stale completions; references contain no process pointers. Adapters never reenter Worlds or call graphics APIs. [Current implementation](../../crates/ipp-core/src/services/io) owns buffering and adapters. [Built-ins](../../crates/ipp-core/src/services/asset_management/builtin) use the same loading/recovery path without special World or renderer behavior.

## Shader recipes and graphics loading

Immutable shader recipes specify compilation features and required passes; component values never create program variants. Renderer-owned providers compile/link programs and upload meshes before graphics readiness. Built-in/custom programs share this lifecycle; headless Hosts may retain authored recipes without claiming GPU readiness.

Hosts register narrow device/compiler dependencies and progress loaders with the correct context outside World evaluation. Loaders never borrow Worlds or locate services through World state. Retain only handles/layout metadata and CPU data required by actual consumers; release temporary decode/compile inputs. [Graphics loaders](../../crates/ipp-render-gl/src/services/render) own representation details.

## Client-authored sources

Clients register immutable names in session-isolated scene scopes; scene-local IDs are authoring bindings, not mutable resource aliases. Keep the I/O source available while producer ownership or consumers require it. Explicit preparation loads before component selection, allowing editors to keep prior selections until replacements are ready. Release removes only that producer's ownership/demand. Registration, decoded data and GPU allocation have independent lifetimes.

## Column-binding definitions

[Column-binding definitions](data.md#data-bindings-and-evaluation) are ordinary immutable, reusable assets. Data-source bindings are ordinary consumers of them.

## Geometry definitions

Authored/generated immutable [bounding and picking definitions](rendering.md#bounding-and-picking-geometry) belong to assets; instances own evaluated shapes. Skeletal definitions depend on mesh, skin binding and skeleton together. Fitting belongs with implementation.

## Surface resources

Surfaces consume immutable font and drawing assets with shared quadratic contour data. Fonts preserve glyph identities and headless layout metrics; drawings preserve ordered painted paths and fill rules. Conversion owns source-format interpretation and approximation, while the renderer owns acceleration structures, curve textures and device-specific packing. Curve textures keep contour coordinates exact, using the narrowest [fixed-point texel format](../../crates/ipp-render-gl/src/services/render/surface_path.rs) that represents them. Bitmap resources preserve colour and coverage alpha independently, with explicit colour conversion under the rendering contract.

[GUI text and skins](gui.md#text-and-skins) consume these same resources and ordinary animation clips. GUI adds no separate loading or asset-identity system; font measurement remains headless, and skin values stay independent of immutable resource content.

## Pending resources and rendering

Producers may declare immutable source identities before producing bytes. The source provider owns the pending index and availability notifications; pending readers wait without occupying active transfer slots. Ready and failed notifications release those waits, while decoded and graphics readiness remain separate asset lifecycle states. Producer disconnection fails unresolved reads instead of silently retargeting them.

Valid references commit independently of readiness; I/O/failure never blocks or rolls back them. Partial/invalid payloads are unavailable. Missing required data skips affected work while ready items continue; later readiness needs no resubmission. Preparation before selection does not make batches atomic. GPU failure cannot publish partially usable graphics data and preserves usable CPU geometry and sessions.

## Data-plane ownership

Asset payloads travel exclusively through the data plane, including client-authored immutable sources. World commands carry references and never embed asset bytes. Source delivery and its acknowledgements progress independently of World command batches; registration does not imply decoded or graphics readiness. Bulk transfers hand exclusive ownership to the Host. Retained decoded data cannot borrow transient input. Shared memory requires synchronized immutable access and explicit release before reuse; cross-process descriptors imply no usable pointers.

Mapped and shared source ranges remain valid and immutable throughout each read. Publication and release fence producer writes, unmapping and storage reuse; cancellation and source replacement prevent new reads without invalidating an active borrow. Descriptors identify an exact source lifetime and checked byte range, never a process address. Direct access requires memory addressable by the consuming runtime: an arbitrary JavaScript buffer is not WebAssembly linear memory. Where direct access is unavailable, the Host adapter fills reusable reader-owned memory addressable by the runtime and exposes it through the same window contract, without assembling another whole-source buffer.

Backpressure bounds chunks, not total asset size. Use checked lengths and fallible allocation where supported. Loaders decode incrementally into private destination storage and publish only after complete validation; whole-input buffering requires a format-specific need rather than being the default adapter. Streaming does not promise zero-copy decoding or eliminate representation conversion.

## Retention and recovery

Identity, CPU availability and GPU residency are independent. Unload clears payload availability while retained references preserve identity; [invalidation](#asset-lifecycle-propagation) precedes release. Reload always reopens the same immutable source through I/O and fails explicitly if it is unavailable or cannot reproduce the same content. The asset layer does not retain original encoded bytes for recovery. Source storage and caching belong to I/O providers, including browser caching; decoded CPU data is retained only for actual consumers.

Completed output publications hold explicit leases for resource identities and the exact immutable CPU data or recovery sources their derived contributions need. These leases outlive mutable component edits and keep frozen output recoverable without rereading changed World state. GPU allocations remain separately reclaimable and rebuild from the retained publication and the same immutable resources. Leases do not override explicit unload, source revocation, destruction or session cleanup: invalidate affected published work before release or reuse, suppress unavailable contributions and report recovery failure instead of retargeting another producer. A frozen branch cannot postpone this barrier.

Losing the last consumer does not immediately discard successfully loaded immutable content with a valid external recovery source. Retain it while the unused-resource cache has space, and reclaim idle entries under pressure using both their measured size and recency of use. Retention works without configuration: the Host cache has a non-zero default soft target that Hosts may raise, or set to zero to evict on release. Returning demand can reuse cached content without another acquisition. Cache ownership never grants a World access to another producer's private sources, retains abandoned transfers, or overrides explicit unload, source revocation and session cleanup. Eviction uses the ordinary synchronous invalidation and generation barriers; CPU and graphics allocations remain separately accounted.

Context loss preserves logical state while invalidating GPU work/allocations. Providers reconstruct graphics representations while preserving usable decoded data/metadata even after failed recovery. CPU consumers test decoded availability independently of graphics readiness; account CPU retention and GPU allocations separately. [Rendering](rendering.md#residency-and-recovery) owns context execution/failure scope.

Browser source delivery and Host-owned decode/graphics loading progress independently of simulation, with concurrent readers and bounded chunks. The Host keeps graphics progress behind its live-context barrier, while World-visible outcomes and events remain frame publications. Inactivity excludes intentional backpressure. Native filesystem access requires an explicit root/prefix and is disabled by default.

## Asset output and durable bundles

### Client read authority and output lifetime

A source identifier grants no client read authority. The Host issues connection-scoped read capabilities for that connection's own producer registrations and explicitly granted sources. Each capability names the exact source registration, asset type/variant and allowed representations; a URI, asset key, World session or component reference alone grants no export access. A capability conveys authority without retaining source or working data; active reads own their required backing. Host policy may explicitly expose public sources or grant sharing. Internal filesystem/network loading authority is separate, and replacement registrations never inherit a capability.

Completed captures, saves, contracts and fully encoded typed exports become detached immutable Host outputs. Later World destruction, asset unload or source revocation does not invalidate an already published output lease. Original-source reads retain their exact I/O registration and grant: explicit revocation stops future reads, while decoded/GPU unload alone does not. Unfinished typed exports fail if a required working representation is unloaded. Producer release removes only that ownership. Existing borrowed windows and queued delivery bytes retain their lifetime until their respective release/completion boundaries.

Original reads reopen the authorized immutable I/O source without acquiring decoded/GPU data. Typed exports explicitly select a supported working CPU/GPU representation and versioned encoding, using existing asset encoders where available. Asset types own semantic encoding; renderer exporters own asynchronous GPU readback, synchronization and conversion. Device handles, compiled shader binaries and packing are not asset formats. Hosts report supported combinations and fail explicitly for unsupported formats, absent representations, context/device failures or revoked access, without silently substituting a representation. Successful typed export is re-loadable, without promising original-byte equality, encoded recovery retention or stall-free GPU readback.

Core provides executor-independent asynchronous output and typed encoding; Hosts own destinations/publication. Output backpressure, cancellation and errors stay separate from loading and World outcomes. Publish only complete output, preserving existing destinations on failure where writers promise atomic publication.

[World saves](protocol-and-schema.md#snapshots-and-world-replacement) preserve references without acquiring, packing or rewriting assets. Bundling remains a deferred operation that relocates assets and updates references before ordinary serialization; there is no asset-gathering save job.
