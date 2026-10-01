# React Reconciliation Strategy

[Authoring](../architecture/authoring.md#react-declarations) · [Core lifetime](../architecture/runtime.md#entity-lifetime) · [Sessions](../architecture/protocol-and-schema.md#snapshots-and-world-replacement) · [React package](../../packages/ipp-react/README.md)

## Declarations and composition

Diff committed props against the target World's selected manifest and write changed fields directly under the [authoring contract](../architecture/authoring.md#react-declarations). Record the entities and components each root created or adopted so that removal and unmount delete exactly those (on referenced entities, only the components the root inserted), and adopt existing entities by declared symbolic id on mount. Use core link declarations for explicit parenting and reordering, resolving conflicts against acknowledged actual entities. Keep multiple roots targeting the same World and browser composition optional.

Preserve existing same-World declaration scopes and introduce a distinct attached-World boundary. Track the attachment, the borrowed or created child World lifetime and child declarations separately. Use multiplexed sessions without changing the Canvas's root output when a child session opens. Exercise teardown and replacement across DOM/scene context, Suspense and error boundaries with the real reconciler.

Collect asset declarations and references across the root before submission. Prepare immutable replacements before switching consumers; keep value edits independent of shader or clip revisions. Bind animation after target acknowledgement and resource readiness, preserving playback through ready replacements. Package source and maintained examples own APIs.

Translate raw Surface content into ordinary Canvas entity declarations and preserve keyed identities across order, content and style edits. Unchanged and callback-only renders send no structural operations; local edits touch only affected declarations. Gallery interaction should request only the state it consumes, bound outstanding view-qualified projection work and preserve click/disposal fencing while the Host evaluates animation.

## GUI declarations

Make the optional GUI entry point emit ordinary entity/component/link declarations under the [GUI client contract](../architecture/gui.md#client-and-persistence-boundaries). Preserve keyed entity and compact part identities through reorder and theme changes. Declare shared theme entities and per-control overrides without expanding theme values into every control. Write declared control values on first commit and whenever the prop changes; observe values through field subscriptions and change them through actions or compare-and-set. Browser mounting consumes the reusable input adapter routed through the presented root.

Extend real worker/WASM/WebGL React scenarios with partial acknowledgement, StrictMode/unmount, delayed callbacks, compare-and-set conflicts, theme changes during editing, session replacement and reconnect adoption. Pin momentary-effect callback ancestry to its effects, deliver value callbacks from field observations and assert exact-incarnation refs, path/order and structural write counts alongside values and completed frames. Exercise focus, capture, clipboard and IME routing through nested output paths and cancellation when those paths become unavailable; retain actual platform evidence separately from synthetic events.

## Commit and acknowledgement handling

Track desired declarations, submitted work and acknowledged identities separately. Submit a commit's creations, adoptions, links and entity-valued fields as one logical batch that names entities by batch aliases or symbolic ids. Replace superseded unsent render descriptions within one pending scheduler position; promises for those renders settle with the description actually applied at that position. Keep imperative commands, resource readiness, validation failure and teardown as ordered boundaries, and retain enough state to clean up accepted work after unmount or partial failure. Corrected renders reconcile from actual applied identities; when the applied extent is unknown or a render fails, delete what the records hold and recommit, reaching uncertain entities through their symbolic ids. Fence callbacks and delayed completions by declaration and session. Reconnection adopts existing entities by symbolic id; fresh sessions still use fresh runtime handles.

Resolve child World/session and output identities before parent attachment submission. Recover from a successful batch in one World followed by failure in another without assuming rollback; cleanup retains the acknowledged identities needed to detach, delete what the root created or adopted and destroy only child Worlds the boundary created. Keep asset preparation and World-local animation binding behind their existing readiness/acknowledgement boundaries.

## Integration harness

Extend the `react`, `canvas`, `animation` and `custom-materials` [suites](../../tools/pipeline/suites.json) through real React DOM/custom reconciler → generated SDK → worker/WASM/WebGL. Use shared World/asset fixtures and a second client writing the same fields as React.

Assert acknowledged identities, last-write-wins against the other writer, deletion of exactly what React created or adopted, reconnect adoption, controller continuity and completed pixels. Gate real transport/resource delivery to exercise cross-World partial failure, superseded replacements and pending cleanup. Composition fixtures cover multiple roots in one World, same-World scopes, attached Worlds, multiple canvases and StrictMode; gallery interactions supply application evidence.

Verify unchanged, callback-only and style-only content produces no structural writes, while content edits and reordering retain entity identity. Exercise rapid pointer movement, hover reversal and direct clicks through real gallery transport, retaining request counts and completed-frame assertions. Keep the [React GUI stress workload](../../tests/performance/gui-stress.md) under the [runtime benchmark strategy](runtime-and-rendering.md#gui-implementation-approach) without reducing its logical work.

Keep assertions separate from launch, transport and capture drivers so external Hosts can reuse them. Follow the [testing policy](../development/integration-testing.md) for readiness, frame barriers, artifacts and owned-participant cleanup. Detailed cases belong with maintained tests.
