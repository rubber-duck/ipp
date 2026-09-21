# React Reconciliation Strategy

[Authoring](../architecture/authoring.md#react-declarations) · [Core lifetime](../architecture/runtime.md#component-modes) · [Sessions](../architecture/protocol-and-schema.md#snapshots-and-world-replacement) · [React package](../../packages/ipp-react/README.md)

## Declarations and composition

Diff committed props into sparse declarations against the receiving target contract. Keep browser composition optional and cross-renderer context boundaries explicit. Reuse core lifecycle and fallback policy rather than repairing state in React.

Collect asset declarations and references across the root before submission. Prepare immutable replacements before switching consumers; keep value edits independent of shader or clip revisions. Bind animation after target acknowledgement and resource readiness, preserving playback through ready replacements. Package source and maintained examples own APIs.

Retain encoded Surface structure when item identities, order and content are unchanged, including commits that alter only style properties. Keep collection encoding independent of incidental React rerenders. Gallery interaction should request only the state it consumes, bound outstanding hover projection work and preserve independent click/disposal fencing while the Host evaluates animation.

## GUI declarations

Extend the existing reconciler with the optional GUI entry point under the [GUI client contract](../architecture/gui.md#client-and-persistence-boundaries). Keyed nodes and named parts retain acknowledged identities through reorder and skin changes. Keep local control initialization separate from explicit revision-checked replacement/reset; distinguish desired structure from runtime value observations rather than replaying props as input state. Browser mounting consumes the reusable platform adapter in the web entry point.

Extend real worker/WASM/WebGL React scenarios with partial acknowledgement, StrictMode/unmount, delayed callbacks, stale value replacements, theme changes during editing and session replacement. Assert node identity, logical callback path/order and structural write counts alongside committed values and completed frames. Consume the runtime/browser suites' focus, capture, IME and touch evidence; do not duplicate control evaluation in React or replace platform evidence with mocked events.

## Commit and acknowledgement handling

Track desired declarations, submitted work and acknowledged ownership separately. Replace superseded unsent render descriptions within one pending scheduler position; promises for those renders settle with the description actually applied at that position. Keep imperative commands, resource readiness, validation failure and teardown as ordered boundaries, and retain enough state to clean up accepted work after unmount or partial failure. Corrected renders reconcile from actual applied identities. Fence callbacks and delayed completions by declaration and session. Verify current reconnection support before extending it; fresh sessions require fresh runtime handles.

## Integration harness

Extend the `react`, `canvas`, `animation` and `custom-materials` [suites](../../tools/pipeline/suites.json) through real React DOM/custom reconciler → generated SDK → worker/WASM/WebGL. Use shared World/asset fixtures and a second producer editing base state beneath React overlays.

Assert acknowledged ownership, latest-base reveal, controller continuity and completed pixels. Gate real transport/resource delivery to exercise failures, superseded replacements and pending cleanup. Composition fixtures cover multiple canvases, nested scopes and StrictMode; gallery interactions supply application evidence.

Verify that unchanged and style-only Surface declarations produce no structural writes, while content edits and reordering reach the runtime with stable identities. Exercise rapid pointer movement, hover reversal and direct clicks through the real gallery transport; count the relevant requests and retain completed-frame assertions so reduced traffic does not hide missed interaction or stale presentation.

Keep assertions separate from launch, transport and capture drivers so external Hosts can reuse them. Follow the [testing policy](../development/integration-testing.md) for readiness, frame barriers, artifacts and owned-participant cleanup. Detailed cases belong with maintained tests.
