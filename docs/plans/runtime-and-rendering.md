# Runtime, Assets and Rendering Strategy

[Runtime](../architecture/runtime.md) · [Assets](../architecture/assets.md) · [Rendering](../architecture/rendering.md) · [Workspace](../architecture/rust-workspace.md)

## Host execution

Extend shared session policy and native/browser adapters through generic Host/World interfaces. Keep composition separate from subsystem behavior. Use real clients to exercise multi-World scheduling, attachment, pause/resume, congestion and teardown. Observe healthy-peer progress under gated delivery and noisy senders.

## Evaluation strategy

Extend the [System lifecycle and dependency interfaces](../../crates/ipp-core/src/world/systems) under the [semantic schedule](../architecture/runtime.md#frame-order). Keep instance results in components and subsystem bookkeeping in Systems. Test release barriers and prepared-output invalidation independently of update order.

Use maintained animation, hierarchy, skinning and mesh-pose fixtures before optimizing sampling or storage. Compare evaluated results with independent baked references. Extend bounds and picking together while keeping conservative enclosure distinct from interaction fitting. Additional pose-changing operations require the accepted dependency model; physics or iterative solving needs architecture review.

Compile real-time typed access and dependency selections at mutation/preparation boundaries, retaining work buffers and batching derived-result invalidation. Compare lifecycle, mixed-controller failures, pointer safety and independently sampled poses across native and WASM. Measure advancing updates separately from rendering; keep profiling instrumentation opt-in without changing production runtime paths.

Extend controller playback with signed clocks and explicit transitions under the [animation contract](../architecture/runtime.md#animation-and-constraints). Keep ordinary single-clip sampling efficient; prepare transition target unions, independent samplers and typed output access only while needed. Retain sparse underlying values and bounded interrupted origins through target or asset lifecycle changes. Extend core persistence and generated protocol/client contracts together, with observable transition progress and no client frame loop driving blending.

Use the maintained `animation`, `skinning`, `gallery-gui` and `snapshots` suites through real native WebSocket and worker/WASM clients. Independent numeric and joint-pose fixtures should cover reverse endpoints and loops, phase-aligned locomotion, partial gesture transitions, interruption, delayed assets and a saved interrupted fade. Completed WebGL frames and state/events must agree with independent sampling expectations. The Surface hover scene exercises a single reversible clip in a real pointer flow; existing drivers keep these scenarios independent of Host arrangement. Signed clocks and transition math also need focused core tests and malformed protocol coverage.

## Asset transport

Extend generic sources/readers/writers and Host adapters together through the [asset boundary](../architecture/assets.md). Measure copies and retained representations before adding transports or shared memory. Keep World dependency accounting separate from resource loading, including applied effects of failed batches.

Exercise client-authored sources over a distinct bounded data-plane protocol on both worker and native connections. Verify immutable names, complete-only publication, session fencing, interrupted delivery, payloads larger than command framing, and source progress while a logical command batch withholds World evaluation. Use the same resource lifecycle and image assertions as externally acquired assets.

Graphics providers receive narrow device dependencies at Host registration. Remove redundant representations only after identifying actual CPU consumers and recovery sources. Built-ins and new formats use ordinary ingress and lifecycle paths.

Separate loss of consumer demand from destruction of reusable loaded content. Add Host-configurable idle retention through the existing catalog and release barrier, accounting measured bytes and ranking pressure-driven eviction by both size and recency. Validate repeated clip selection without reacquisition, active/shared demand protection, forced pressure and session-private source teardown. Gate real browser asset delivery while observing advancing animation poses and completed frames; fast local fixtures alone cannot establish smooth mode changes.

## Rendering and recovery

Extend shared WebGL/GLES preparation and renderer-owned loading. Reuse final headless camera/geometry state; add acceleration only for measured workloads. Keep light selection, shadow allocation, material packing and device limits private to rendering.

Exercise completed frames under resource delay, allocation failure, material fallback and actual context loss. Distinguish draw/resource failures from context, World and Host failures. Verify usable CPU interaction and healthy Worlds survive graphics failure. Extend custom shader and transparency cases through existing material/lifecycle paths rather than separate loaders.

Retain prepared geometry, material state and draw records; borrow evaluated transforms within their read phase. Compare flat and hierarchical batch queries against identical conservative candidate sets and completed frames. Use overview and interior camera views with unchanged animation load, and report far-plane changes separately. Verify cache invalidation through resource, component, World and context lifecycle changes. The [performance harness](../../tests/performance/README.md) stays outside regression selections.

## Build composition and size

Build matching headless/rendered baselines and representative optional capabilities. Inspect resolved features and measure WASM, JavaScript/shaders and native shims separately. Validate capability omission and packaged shims under the [workspace policy](../architecture/rust-workspace.md); broad compilation alone does not establish either.

Use the Python pipeline's declared products, shared executor and selected environment checks for all maintained commands. Keep Node operations limited to their actual JavaScript/WASM work. Validate clean-checkout prerequisites, matching generated targets, per-run manifests, failure/cancellation cleanup and representative native/browser/GLES scenarios. Build products may be reused by scenarios and examples without coupling their fixture bundles. Keep source and environment identity explicit before reusing validation evidence.

## Particle implementation approach

Extend private CPU evaluation, immutable cache decoding and renderer-owned instance uploads through existing animation, asset and material paths. Use seeded emitters and sampled playback fixtures to compare lifecycle, seeking, bounds and rendered output. Blender translation follows the [authoring strategy](blender-integration.md).

## Surface implementation approach

Align raw Surface authoring and GUI on the [shared top-left, Y-down content convention](../architecture/rendering.md#surface-presentation). Update Surface preparation, glyph placement, drawing/bitmap orientation, clipping, animation inputs and plane interaction together; keep entity placement and World conventions at the common Surface boundary. Regenerate affected converters/assets, clients, examples and expected fixtures directly, without a legacy coordinate mode. Use asymmetric content and independent corner/line-position assertions through real native/worker clients and completed WebGL/GLES captures to expose double flips, winding errors and hit/paint disagreement.

Extend typed component-owned collections through existing numeric-property, StateOverlay, asset-demand and persistence paths. Compile item bindings independently of painter's order, retain headless string layout and accept externally positioned glyph runs. Convert fonts and the supported SVG subset offline into shared quadratic contours; prepare their renderer-specific acceleration and uploads through ordinary providers. Integrate Surface submissions, RGBA bitmaps and rectangular clipping in the shared WebGL/GLES path.

Keep item edits proportional to affected content and properties. Retain prepared glyph and renderer data across unrelated camera or style changes, preserve painter order, and invalidate retained data before identities or resources are reused.

Implement [GUI shapes and retained presentation](../architecture/rendering.md#gui-shapes-and-retained-presentation) by extending the existing box primitive and stable skin parts. Carry evaluated shape dimensions and bounded fill, border, gradient and glow inputs through the shared preparation boundary; regenerate target-correct authoring contracts and preserve ordinary animation ownership. Use explicit non-indexed triangle batches, with narrow outline/corner coverage where it saves fragment work. Keep the current SVG/quadratic and bitmap paths interoperable in the same ordered stream, without converting ordinary controls into mutable drawing assets.

Retain CPU geometry by primitive identity and material/geometry revision, and replace complete changed GPU batches while preserving storage used by queued draws. Start with device storage replacement and measure an explicit pool only if needed; avoid assuming orphaning guarantees stall-free allocation or that a fixed frame delay proves storage reusable. Choose bounded batch granularity so cursor or control animation does not rebuild every panel. Batch boundaries must preserve clipping, painter order and resource compatibility; camera placement updates use shared transforms.

Implement [glyph presentation](../architecture/rendering.md#glyph-presentation) with shared coverage atlases populated from existing font contours and explicit glyph triangles. Reuse headless metrics and positioned runs. Compare raster resolution bands against direct curves using actual small-text and oblique-view captures before choosing defaults; preserve current analytic fallback. Keep atlas locations stable while referenced, pad/filter entries without neighbour bleed, and invalidate dependent geometry before eviction or reuse. Batch cache misses and bound generation/allocation work. Evaluate MSDF separately if the required scale range warrants its generation and shader costs; neither path implies full Unicode shaping or Ghostty integration.

Implement [optional texture caching](../architecture/rendering.md#optional-surface-texture-caching) behind the shared RenderService/device boundary, using a whole Surface as the unit of rasterization and repaint. Extend prepared inputs with enough content identity and interaction priority to distinguish paint changes from placement changes without a second authoritative GUI store. Cover raw Surface and GUI output, including transient paint and asset readiness, through the same invalidation boundary. Reuse the ordinary curve, bitmap and box rendering paths to build GPU images without CPU readback, and composite them with the existing Surface placement and transparency rules.

Use configurable camera-distance bands to select direct presentation or cached resolution and refresh cadence, with stable mode transitions. Coalesce changes until a refresh is due and keep focused or actively interacted-with GUIs direct. Bound image allocations and preserve direct rendering as a fallback; retirement and context recovery must release derived state and rebuild only from live evaluated inputs. Keep detailed policy values, allocation algorithms and API fields beside the implementation.

Use converted local font/SVG fixtures and a terminal-style scene through generated browser and native clients. Combine state and mutation-count assertions with completed WebGL/GLES captures for incremental edits, animation, lifecycle, persistence, delayed resources and recovery. Measure unchanged, camera-only and item-edit workloads separately from asset preparation; keep hardware timing separate from deterministic work counts.

Extend the maintained GUI gallery with a reusable neon control specimen covering resized borders, gradients, localized glow, small labels and complex curve icons. Exercise default, hover, press, focus and disabled paint through existing controls and Host-driven animation. Through production worker/WASM/WebGL and native/GLES drivers, assert meaningful completed-frame regions, clipping, overlapping transparency, paint/hit agreement and context recovery, including mixed shape/text/curve order. Keep fixtures owned by examples and scenario assertions independent of launch and wire layout. Record retained batch rebuilds, uploaded bytes, glyph misses and resident bytes through narrow diagnostics, with zero unchanged-content uploads after warmup as an observable invariant.

Extend the maintained terminal fixture with viewport-bounded glyph runs, typing, cursor blink, scrolling and full-screen updates, reusing atlas entries across repeated panels. Coalesce paint preparation to rendered frames while preserving all semantic updates. Compare warm and cold caches, idle and continuously changing scenes, sparse and full geometry updates, and bounded memory under churn. Use controlled camera/viewport/DPR and report source, device, OS/browser, warmup and interval identities. Real iPhone measurements establish mobile performance; software graphics and frame-cadence counters alone do not establish GPU timings. Partial Surface damage redraw remains a measured follow-on: it must account for old/new glow bounds and replay intersecting content in order, including the bandwidth cost of preserving target contents.

Extend that harness with near and distant Surfaces containing static labels, translucent overlapping drawings, clipped controls and Host-driven animation. Drive distance and focus transitions through production clients in worker/WASM/WebGL and native/GLES environments, and compare completed frames with direct presentation at controlled states. Observe cache dimensions, repaint counts and resource lifetime through narrow diagnostics; prove content reuse, distance-dependent quality, current interaction feedback and continued World progress. Reuse the scenario assertions across environment drivers, retain failure captures and clean up all owned participants. A repeated-panel performance fixture should record real device identity, rendering work and memory alongside timing, so software-rendered correctness is not presented as mobile GPU performance evidence.

## GUI implementation approach

Implement the [GUI boundary](../architecture/gui.md) through subsystem-owned mutation/control, layout and input passes. Extend the existing component and dynamic-property lifecycle, generated contracts and retained Surface preparation; keep node/part identity independent of paint storage. Publish coherent implementation interfaces before dependent work. The System hooks currently separate ingress, restoration, preparation and evaluation; establish next-boundary action enqueueing and pre-animation skin request consumption without GUI branches in World/Host orchestration or mutable cross-System references.

Extract reusable headless font measurement from Surface label preparation before adding constrained wrapping and grapheme/caret mappings. Prepare layout, paint and hit regions from the same evaluated inputs, intersect rectangular clips once, and preserve ready output across unrelated changes. Extend the shared WebGL/GLES path for clips and parameterized backgrounds without making core depend on rendering.

Extend maintained Surface/native WebSocket and worker/WASM/WebGL drivers with generated GUI clients, local font/drawing/bitmap/clip fixtures and a settings panel containing wrapped text, two skins, editable controls, nested scrolling and an explicit scene blocker. Register reusable GUI scenarios in the suite/build catalogs as implementation lands. Assert source/effect ticks, ordered multi-event input, revision conflicts, lifecycle cancellation and independent expected hit/paint geometry; capture completed WebGL and focused GLES frames, including clipped text/backgrounds and recovery. Record work counts for unchanged frames, camera-only changes and local edits separately from timings.

Keep browser input adapters reusable with a plain generated client. Delay application callbacks while actual ingress continues to prove runtime ownership. Extend browser drivers for resize, capture, focus loss and composition; retain separate actual OS IME and touch-device soft-keyboard evidence, since synthetic events cannot establish those platform behaviors. Future process arrangements reuse scenarios through new drivers. Follow the common harness policy for readiness, frame barriers, failure artifacts and cleanup.

## Integration harness

Extend the [suite registry](../../tools/pipeline/suites.json) and maintained native WebSocket, worker/WASM/WebGL and native GLES environments with real generated clients and local resource/rig fixtures.

| Area | Observable proof |
| --- | --- |
| Composition/lifecycle | Deterministic dependency order, rejected graphs, invalidation before reuse and no stale outputs |
| Shared resources | Two Worlds share acquisition while retaining independent demand and teardown |
| Evaluation | Animation, hierarchy and constraints agree with independent final-pose references |
| Geometry/rendering | Headless queries, conservative bounds and completed frames agree through recovery |
| Async failures | Gated delivery exposes pending work, cancellation and session fences without client repair |

Keep fixture/assertion intent separate from process, transport and graphics setup; new arrangements add drivers. Follow the [testing policy](../development/integration-testing.md) for readiness, frame barriers, failure artifacts and cleanup. Focused lifecycle/compiler tests supplement real paths; detailed case inventories remain with maintained tests. The [Blender harness](blender-integration.md#maintained-integration-harness) adds authoring participants.
