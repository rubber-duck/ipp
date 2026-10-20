# React world components

`@ipp/react` turns committed React declarations into ordinary IPP entity, component and link writes. Applications compose props; the core owns lifetime, required components and evaluation. Definitions resolve against each receiving generated client without retaining session identities.

Build with `python tools/ipp.py build react` from the repository root. Import scene declarations from `@ipp/react` and browser composition from `@ipp/react/web`. React is a peer dependency; the client and reconciler are package dependencies. Applications supply React DOM. [package.json](package.json) owns versions and exports; [index.ts](src/index.ts) defines the public entry point.

## Data sources and bindings

Headless data declarations compose ordinary entity components with Host-wide datasets. [Data helpers](src/data/declarations.ts) expose buffer and streaming bindings, immutable column definitions and explicit source ownership. Use the receiving target's generated `ExpressionBuilder` and `encodeDataWindows`; definition assets follow the same preparation, failure and replacement behavior as other root-local assets.

```tsx
import {
  createRoot, DataSource, Entity, ColumnBindingAsset,
  StreamingDataSourceBinding, assetRef,
} from "@ipp/react";
import { ExpressionBuilder, encodeDataWindows } from "./generated.js";

const expression = new ExpressionBuilder();
const sample = expression.input("column:value", "f32");
const gain = expression.input("parameter", "f32");
const definition = expression.encode(expression.binary("multiply", sample, gain));
const root = createRoot(client);
const name = "datasets://sensor";

await root.render(
  <DataSource name={name} ownership="producer" kind="streaming"
    schema={[{ name: "value", kind: "f32" }]} datasets={host.datasets}>
    <ColumnBindingAsset id="scaled" definition={definition} />
    <Entity id="sensor-view">
      <StreamingDataSourceBinding source={name}
        columns={{ scaled: { definition: assetRef("scaled"), parameter: 2 } }}
        windows={[{ kind: "count", count: 100n }]} encodeWindows={encodeDataWindows} />
    </Entity>
  </DataSource>,
);
const producer = root.getDataSource(name)?.handle;
if (!producer) throw new Error("Source preparation failed");
const outcome = await producer.update([
  { operation: "append", rows: [[{ kind: "f32", value: 3 }]] },
]);
if (outcome.failure) throw new Error(outcome.failure.reason);

// Explicit declaration removal releases producer ownership and removes the entity.
await root.render(null);
await root.unmount();
```

`BufferDataSourceBinding` uses the same `source` and `columns` without windows. Identity definitions encode one `ExpressionBuilder.input("column:<name>", kind)` as the output. A column can also reference an already registered concrete asset. Parameters are ordinary dynamic values named `<output>_parameter`, so existing animation declarations can target them; parameter changes do not replace definitions. Omitted columns and parameters preserve their last stored value, as ordinary React props do. Use `parameter: null` to remove a parameter (allowing an expression's missing-input fallback), or `columns: { output: null }` to remove that output and its companion parameter.

`DataSource` with `ownership="borrowed"` names an existing source and never creates, releases or destroys it. Producer declarations create through the existing `ClientDatasets` lane after commit; their handles send ordered bulk deltas with that lane's backpressure and partial-outcome behavior. Data rows stay in the Host, outside React and World command batches. Removing a producer declaration releases only producer ownership; explicit destruction remains `host.datasets.destroy(handle.producer)`. Root unmount preserves sources and authored World state, and fences the helper's handles. The Host connection still owns the preserved producer; retain its token for deliberate later update/release/destruction.

`getDataSource` and `onDataSourceChange` report producer preparation, readiness and failure. Borrowed state reports ownership only; source and evaluated binding availability are separate observations through `host.datasets.read` and `host.datasets.bindingView`. Those read-only observations never clear binding dirty. Await `root.render` before streaming samples needed by its windows; it establishes binding acknowledgement, while expression asset readiness and evaluated columns remain separate barriers. The maintained [React data scenario](../../tests/data/scenarios/react-data.ts) runs through native WebSocket and worker/WASM in the `datasets` suite.

## Data charts

[Typed Plot declarations](src/plots/README.md) compose a data binding, frame and chart on one Entity, preserving source-row label identities and stable series slots through the receiving target's generated row codec. The [native chart showcase](../../examples/chart-showcase/README.md) authors both reference studies with real sources and Host-controlled parameter animation.

`PlotLegend` composes a caller-authored categorical key or numeric color scale from ordinary Canvas leaves. Its placement helper keeps the key outside chart bounds in canvas coordinates or a scene's local XY plane; callers retain ownership of the chart, Surface and Canvas World. [The Plot authoring guide](src/plots/README.md#color-legends) explains shared color mapping and composition.

## Declaration roots

Connect a matching generated client:

```tsx
import { createRoot, Entity, Scalar } from "@ipp/react";

const root = createRoot(client, { onError: console.error });
try {
  await root.render(
    <Entity bindTo="producer">
      <Scalar value={3} />
    </Entity>,
  );
} finally {
  await root.unmount();
}
```

This writes a Scalar on an existing entity named by its symbolic id. `Entity` generates a root-scoped identity when `id` is omitted. Use `id` to create or adopt a persistent symbolic identity, or `bindTo` to reference an existing entity that the root neither creates nor deletes. React keys preserve component-local identities through reorder; explicit symbolic ids retain native identity even across a keyed remount. A render may declare each `id` only once: two `<Entity id>` nodes with the same id reject the render locally with `ReactWorldDuplicateEntityError`, naming the id, before anything is sent. An id that moves to another node between renders (a keyed remount, or a declaration moved elsewhere in the tree) keeps its entity. Several `bindTo` nodes, including one beside the `<Entity id>` in the same render, may refer to one entity; a component that another of them still declares stays when one declaration of it is removed. A component's first commit inserts it with its declared fields, or writes those fields in place when it already exists; later renders write changed fields. The last write wins: removing a prop leaves its last value, and no earlier value is restored. Removing a declaration while the root is mounted deletes what it declared: a removed `<Entity id>` deletes its entity and a removed component node removes its component, whether the root created or adopted them; a removed `<Entity bindTo>` deletes nothing, though its removed component nodes still remove their components. Unmounting deletes nothing: content that must disappear with a client lives in a World the client creates (temporary if it should end with the connection), and an author who wants cleanup removes the nodes before unmounting. Parenting is explicit through `Children` or `EntityLink`; nesting alone grants neither parenting nor lifetime. [components.ts](src/components.ts) defines props; the [core lifetime contract](../../docs/architecture/runtime.md#entity-lifetime) governs references and deletion.

`root.clear()` removes the current declarations and waits for acknowledgement; it leaves the root usable. Use it before `unmount()` when declaration removal is intended. `createWorldRoot(host, options, rootOptions)` explicitly creates and owns a World, returning a retryable `dispose()` that unmounts, closes its session and destroys that World. The opt-in `canvasWorldOptions`, `guiWorldOptions` and `sceneWorldOptions` presets supply System selections; camera and output selection remain explicit.

`Children` links its direct Entity declarations to the enclosing Entity in keyed sibling order. `EntityLink` instead declares `parent` and optional `before`, each a runtime handle or a unique scene Entity's `id`/`bindTo`; `parent={null}` declares a root and omitted/null `before` appends. These write non-owning core links, not components. Removing a link declaration does not restore an earlier relationship; deleting a parent the root created never deletes other children. Competing link declarations for one actual entity reject within a root after acknowledgement, while independent roots follow last write wins. `ParentJoint` selects a joint against the current parent independently of the link. The selected World manifest governs component and operation admission, not compiled descriptors alone.

Link commits order known parent declarations and `before` anchors before their dependants, including dependencies through unchanged declarations and references resolved by runtime handle. This supports reparenting and chain reversal within the root's declared topology without temporary detach operations. It does not inspect or mirror ancestry written by other clients, other roots or animation: those can still cause core rejection. These edits are ordered, not atomic; corrected renders and unmount retain the ordinary acknowledged cleanup contract.

A camera is an Entity with a `Transform` and a `Camera`; `CameraRig` declares both plus its aim in one node. Give it a `position` and either `lookAt` (a World-space point) or `target` (an entity handle to track): both compose the runtime LookAt System, which owns the evaluated orientation with World +Y up, so no client computes orientation math. `fovY` selects a perspective lens, `orthoHeight` an orthographic one; `near`, `far` and `focusDistance` pass through to the Camera fields. Bind the declared camera through `root.bindOutput("camera-id", "camera")` after acknowledgement; the rig itself selects no presentation.

```tsx
<CameraRig id="main-camera" position={[0, 3, 8]} lookAt={[0, 2, 0]} fovY={Math.PI / 4} near={0.1} far={100} />
```

`render()` acknowledges declarations; `flush()` captures a finite acknowledgement cutoff after the scheduled React commit. It seals pending render slots so later internal hook updates and producer renders cannot replace or extend the captured work. Already-started attachment preparation reserves a position in that record's existing ordered work, covering its initial portal declarations, nested initialization and attachment acknowledgement. Each cutoff retains that position alongside its captured local commits; unrelated parent acknowledgements neither cancel the boundary requirement nor add later work. Boundary generation/lifetime changes fence superseded initialization; retirement-delayed replacements remain outside the cutoff. Rapid renders otherwise replace superseded descriptions that have not started submission, and their promises settle with the description actually applied in that pending queue position. Explicit commands and cleanup preserve their order. Neither method waits for resource readiness or arbitrary application promises. Unmount roots before closing their client. Failed batches may retain partial effects: the applied prefix updates the root's records, and corrected declarations can recover, but unchanged rejected work is not retried. When the applied extent is unknown or a render fails, the root deletes what its records hold and recommits; symbolic ids reach entities whose creation is uncertain. Roots do not migrate between sessions, and general remote refs remain unsupported. A commit's cost follows what React changed: one that only replaces props describes and compares just the declarations React updated, while one that inserts, removes, hides or reveals declarations, or changes what other declarations resolve against, compares the whole tree. See [commit handling](src/reconciler/commits.ts) and [description](src/reconciler/tree.ts).

`unmount()` permanently fences authoring and refs and releases the root's subscriptions and sessions; it sends no deletes, removes no animation controller or asset and neither detaches nor destroys attached Worlds. Concurrent calls share the current cleanup attempt, and a completed successful attempt is reused. After rejection, calling `unmount()` again explicitly retries only retained cleanup; it does not render, recommit declarations or close the shared client. Each returned promise preserves its own outcome, including the original failure. Keep the root until cleanup succeeds or retain the appropriate composition recovery owner when exact remote effects remain unresolved.

## Receiving World services

Pass the receiving target's generated module once with `createRoot(client, { contract })`. Plot rows, data windows and skin providers resolve their encoders from that World scope; explicit overrides remain available. Optional mesh codecs are installed in the same root options with a matching schema hash. Attached Worlds resolve a separate scope against their own client.

`useWorld()` exposes that receiving scope. `useAsset`, `useDataSource`, `useField` and `useCommitStatus` subscribe to acknowledged resource, field and commit state. These hooks report readiness without suspending a render that must commit its own resource declarations. `useEntityId`, `useAssetId` and `useAssetRef` provide stable component-local names, with optional explicit names for external addressing. See [scope types](src/scope/world.ts) and [hooks](src/scope/hooks.ts).

`Resources` keeps shared entities, assets and datasets in React context without linking them into the surrounding native layout. `SkinProvider` uses it automatically, so providers can appear inside layouts. Native controls inherit its font unless an explicit local or native-ancestor font applies.

Mounted content hidden by Suspense retains native state through `PresentationGate`. Complete kit widgets supply an entity boundary; low-level declarations that share an entity need an explicit safe boundary for independent hiding. Ref and input availability follow presentation, while actual removal and root unmount keep their distinct lifetime contracts above.

## Canvas and nested worlds

### Headless attached Worlds

`AttachedWorld` is distinct from a same-World declaration scope. Supply the exact owning Host when creating the root, then declare an explicit anchor and child lifetime:

```tsx
// A camera World names the camera System and every predecessor it requires.
const CAMERA_SYSTEMS = [
  "ipp.animation",
  "ipp.asset-dependencies",
  "ipp.hierarchy",
  "ipp.look-at",
  "ipp.final-propagation",
  "ipp.geometry",
  "ipp.camera",
];
const root = createRoot(parentClient, { host });
await root.render(
  <>
    <Entity id="anchor"><FlatSurface width={1} height={1} /></Entity>
    <AttachedWorld
      anchor="anchor"
      child={{ create: { symbolicId: "child", selectedSystems: CAMERA_SYSTEMS } }}
      attachment={{ mode: "surface-camera", output: { entity: "camera" } }}
    >
      <Entity id="camera"><Camera /></Entity>
    </AttachedWorld>
  </>,
);
```

Use `child={{ borrow: exactWorldReference }}` to preserve a caller-owned World. `attachment={{ mode: "spatial" }}` needs no output. Surface modes require an explicitly authored parent Surface: `surface-canvas` presents the child World's canvas and names no output, and its handle reports `canvasOutput(child)`; `surface-camera` names an exact Camera OutputRef or an entity selector resolved through `Host.bindOutput` after child acknowledgement. String anchors/selectors identify an acknowledged Entity declaration in their own container; bigint selectors are runtime entity handles. Compiled descriptors do not substitute for the selected World manifest. This API does not select a physical root output or provide GUI/input integration.

The child portal has a separate fixed-World container and session, while retaining its parent's React context, Suspense and error ancestry. Nested `AttachedWorld` boundaries are explicit; ordinary JSX and `World` scopes do not change World. Suspense hiding retains the child container so recovery cannot restart asynchronous mounting merely by losing the suspended subtree. No transport work starts in render.

The coordinator opens a narrowly scoped parent receipt session independently of the supplied parent authoring session. It writes only the ordinary attachment component and retains session-scoped receipts through cleanup. It closes that session after its receipts settle. This lets declaration teardown and independent sibling work progress without putting retirement waits on the finite declaration commit queue. Each ready handle's `closed` promise waits for cleanup; `render`/`flush` do not wait for retired publications or arbitrary Suspense promises.

Create/open, child declaration acknowledgement, output resolution and attachment acknowledgement are ordered, not atomic. Journals outlive departing fibers and retain the applied effects of failed batch outcomes; a batch without an outcome remains unknown. Removing a boundary while mounted conditionally detaches exact receipts, preserves foreign replacements, waits for old-token retirement, and destroys only exact Worlds it created after successful cleanup. Unmount and session end release a boundary instead: they settle its submitted work and close its sessions, while the attachment, the child World (even one the boundary created) and the child declarations stay. Superseded work still settles its submitted operations. Refs and delayed callbacks are fenced on removal and terminal session closure.

Creation forwards `temporary` unchanged (the Host default is retained/false). Acknowledged removal of a boundary explicitly destroys the children it created; borrowed boundaries never assign disconnect policy or destroy their child. Connection loss is not evidence of destruction or retirement. Generic closure errors are not parsed: only independent same-Host lifetime observation can establish absence. Otherwise cleanup rejects with `AttachedWorldCleanupError`; its journal reports exact known World/session/receipt identities and unresolved original batch errors rather than inventing missing outcomes. An already-destroyed temporary child is distinct from successful boundary-driven destruction.

`AttachedWorldCleanupError.recovery` owns the still-live receipt session and exposes a current `journal`, `retry()` and `abandon()`. A failed detach does not wait for a token that may remain current forever. Retry serializes another exact cleanup attempt, retaining received effects and re-querying failed retirement observations; it never replays authoring. The original `closed`/`unmount()` promise still records its first failed attempt; await `recovery.retry()` for that boundary's recovery result, or call the root's `unmount()` again to retry its retained cleanup together. Retain the recovery object from the root error callback or cleanup error chain until it resolves. The coordinator closes its receipt session only after every record sharing it has settled or been explicitly abandoned.

Abandonment is explicit relinquishment, not successful cleanup: after a failed attempt, `recovery.abandon()` releases retained receipt handles and owned sessions, returns the remaining identity journal, and never destroys Worlds or writes an attachment. In-flight recovery must settle before abandonment; repeated retries coalesce. Unknown transport effects remain unknown, and callers remain responsible for any retained Worlds they created. A name-reusing creator waits for every outstanding retiring creator of that name in its coordinator, including older replacements in other child containers, outside the root's finite commit queue. Failed cleanup surfaces the original recovery journal instead of attempting an unsafe create. Exact destruction or explicit abandonment removes the barrier; abandonment does not make a still-occupied Host name available. Cancelled unsent successors stop waiting without reserving names. Unrelated roots, active foreign owners and borrowed lifetimes do not enter this name-specific barrier.

The pinned reconciler's portal return declaration omits React's element fields; the local TypeScript conversion reflects the actual React portal implementation, not an alternate renderer or context bridge. [Attachment types](src/composition/attached-world.ts), [coordinator](src/composition/attachment-state.ts) and [real transport scenario](../../tests/react/attached-world/scenarios/attached-world.ts) own the API and lifetime details.

### Canvas Worlds

A World that selects the Canvas System is a canvas; its top-level entities are the canvas content and its extent and density are Canvas System state ([rendering architecture](../../docs/architecture/rendering.md#surface-presentation)). `CanvasWorld` owns and controls one such World:

```tsx
<CanvasWorld
  create={{ selectedSystems: ["ipp.canvas", "ipp.gui", "ipp.gui-layout"] }}
  extent={[320, 200]}
  unitsPerMetre={400}
  presentation={{ anchor: "panel-anchor" }}
>
  <Entity id="enabled">
    <Layout width={160} height={24} />
    <Checkbox label="Enabled" checked={false} />
  </Entity>
</CanvasWorld>
```

The selection must include `ipp.canvas`. `extent` and `unitsPerMetre` set the initial canvas state at creation; later changes send one Canvas state update on the World's session, without a reply, and an omitted prop keeps the current value. Invalid values are refused before anything is created. A changed `create` or presentation creates a new World. `presentation={{ anchor }}` presents the canvas as a SurfaceCanvas child of that parent Surface anchor, through the `AttachedWorld` machinery and its ownership rules; the parent root needs `createRoot(client, { host })`. `presentation={{ root: true }}` needs an ancestor `IppCanvas` (or a root from its handle), creates the World and its own session, and supplies the Canvas root output while `IppCanvas.output` is omitted; an explicit output or a second root CanvasWorld is reported as a conflict. Removing the element destroys its World; closing the Canvas or unmounting the root leaves it to the Host. The ref and `onReady` receive `{ world, output, closed }`, where `output` is `canvasOutput(world)`. [Source](src/canvas/world.ts) and the [real transport scenario](../../tests/react/attached-world/scenarios/canvas-world.ts) own the details.

### Browser composition

`IppCanvas` owns its worker Host and DOM surface. Supply `world={{ create: options }}` or `world={{ load: { url, options } }}` and an explicit `output`: a Camera output, `canvasOutput(world)` for a World that selects the Canvas System, or null; omit it when a root `CanvasWorld` supplies the root. Created Worlds, here and in `AttachedWorld`, name their `selectedSystems`; there is no default selection. Null presents nothing; there is no first-Camera or first-Canvas fallback. World creation options, including `temporary`, pass through unchanged. Graph loading retains every created World identity, including a failed loader's cleanup journal. Configure resource relocation and optional `assetCacheBytes` in `runtime` before startup; the Host keeps up to 64 MiB of unused completed assets for reuse by default, and `assetCacheBytes` overrides that target (0 evicts on release); `browserRuntime(baseUrl)` supplies distribution URLs.

Each nested `World` owns a declaration scope in the fixed authoring World. `World.onCommit(scope)` can call `scope.bindOutput(entity, "camera")` after acknowledgement; binding resolves that Camera's exact identity but does not select presentation. A World's canvas needs no binding: `canvasOutput(world)` names it. Pass the result to `IppCanvas.output`. Selection may name a different World without retargeting any authoring container. `AttachedWorld` retains its explicit portal/context and separate ownership. DOM and scene declarations still have separate React roots: bridge application providers explicitly, and use scene-compatible error/Suspense fallbacks.

`initialize(client, signal, host)` runs before scopes and `onReady`. `onReady` exposes authoring readiness, not a completed draw; `onViewChange` reports an acknowledged physical selection. Callback-only updates do not author components or rebind presentation. Runtime/create/load configuration replacement creates a new owner; output changes and CSS/DPR resizing keep the authoring sessions, and resizing an unchanged output resizes the selected view in one Host request, which keeps the renderer's retained state. Drawing dimensions are negotiated against the actual surface before root binding. `handle.viewport` is the acknowledged viewport, not the latest requested CSS extent.

`flush()` inserts a finite ordered acknowledgement barrier for work already queued by committed React DOM, including its scopes' acknowledgement work. Later renders cannot extend it, and attachment retirement runs outside that queue. A root returned by `handle.createRoot()` has its own ordered `flush` without reentering the Canvas queue. `frame(options)` and `capture(options)` follow that authored-work barrier with the actual Host presentation request, preserving explicit publication/sequence constraints. They do not wait for future renders or imply asset readiness. Captures retain immutable completed-frame identities and bytes; diagnostics are separate and never authorize a frame.

Context loss preserves authoring. Call `recoverPresentation()` explicitly after a fresh context is available: it verifies the exact current root generation and refuses a same-context stale selection or externally replaced root. A known-unsent/typed rejected selection can be explicitly retried with the same output and size, reusing an acknowledged root binding. If recovery acknowledges a resized root but selection rejects, that binding remains authoritative for exact cleanup and explicit selection or recovery retry on the already-acquired fresh context; a successful selection ends this retry permission. Generic Host write errors lack enough outcome information for safe replay and retain an unknown-effect journal instead. No read-after-write inspection substitutes for a root/view acknowledgement, and no cleanup restores a previous selection.

Teardown fences callbacks immediately, settles submitted work, conditionally clears only acknowledged root/view identities and unmounts its roots, which delete nothing, before closing owned sessions and its owned Host; it destroys no World itself. Borrowed Worlds and authoring sessions survive unless session ownership was explicitly granted to the controller. Generic terminal closure is not proof of World destruction or published retirement.

`CanvasCleanupError.recovery` (also reachable as `handle.cleanup`) retains the owner, worker/Host and exact journals after a failed teardown. `retry()` resumes cleanup without reauthoring. `abandon()` explicitly relinquishes cleanup, returns the journal, and may close an owned Host/worker; it does not claim that cleanup completed. It never closes a borrowed Host or destroys its Worlds. Retain this recovery object from the lifecycle error callback even after the DOM unmounts. The original `closed` promise records the first attempt; await the recovery operation for subsequent results. A failed declaration cleanup keeps the root journal available through the same owner.

`CanvasWorldSession({ host, client })` reuses these boundaries with an externally supplied Host and fixed authoring Client, borrowing both by default. [Controller source](src/canvas/world-session.ts), [presentation receipts](src/canvas/presentation.ts), [owner/recovery](src/canvas/lifetime.ts) and [real native/worker scenario](../../tests/rendering/canvas/pages/canvas-controller.tsx) define the implementation. [DOM test pages](../../tests/rendering/canvas/pages/canvas.tsx) exercise both public package variants.

The former GUI relay is not wired into this entry point. The ordinary GUI/input port must preserve presented-path pointer/wheel/capture routing, keyboard and focus handoff, native IME/clipboard, trusted activation, cancellation and generation fences through the accepted input APIs; this presentation slice claims none of that input behavior. Gallery/query/navigation callers remain separate migrations.

## Ordinary GUI authoring

`@ipp/react/gui` supplies ordinary layout, group, overlay, style, box, paint, text/glyph/drawing/bitmap, font, behavior, theme/skin/motion and five control component declarations (Button, Checkbox, Slider, TextInput and Color), beside the scroll controls below. Put components inside `Entity`; use `Children` or `EntityLink` for layout/painter ancestry. There is no GUI root identity or separate reconciler. The receiving World must select the corresponding evaluators and the Canvas System, which makes it the canvas; `CanvasWorld` owns such a World. Configuration follows the generated field names in [the component contract](src/gui/fields.ts).

```tsx
import { createRef } from "react";
import { Entity, Children } from "@ipp/react";
import { Layout, Checkbox, type GuiControlHandle } from "@ipp/react/gui";

// `root` authors a World that selects ipp.canvas, ipp.gui and ipp.gui-layout.
const checkbox = createRef<GuiControlHandle>();
await root.render(
  <Entity id="panel">
    <Layout width={320} height={200} />
    <Children>
      <Entity id="enabled">
        <Layout width={160} height={24} />
        <Checkbox label="Enabled" checked={false} ref={checkbox} />
      </Entity>
    </Children>
  </Entity>,
);
const control = checkbox.current!;
const { checked } = await control.read();
await control.compareAndSet("checked", checked as boolean, true);
await control.action({ kind: "toggle" });
```

Control values are ordinary fields: `selected` on a Button, `checked` on a Checkbox, `value` on a Slider, and `upper` too on a `range` Slider, `hue`, `saturation`, `value` and `alpha` on a Color, and `text` on a TextInput, or `value` on a `numeric` one. A declared value is written on first commit and whenever the prop changes, and the props of one component that change in one commit are written in one component write, validated together, so a range's two values never pass through an inverted state; on reconnect it overwrites the stored value, because the last write wins. There are no initial-value props. [Control handles](src/gui/control-ref.ts) expose the exact World/entity/component-incarnation `target`, `read()` of the control component's current fields, `compareAndSet(field, expected, value)`, which resolves false without effect when the field holds another value, `focus()` and `blur()` convenience actions, and `action(a)`, which submits the semantic action as a one-command `guiAction` batch and resolves with the batch outcome: refused actions fail it with `StaleTarget`, `Unavailable`, `UnsupportedAction` or `InvalidValue` and change nothing. They are not feedback subscriptions. A ref publishes only after declaration acknowledgement and its lifecycle-watch baseline. Removing, hiding or replacing a declaration and terminal client closure fence its handles immediately; already submitted requests retain their protocol outcomes.

Control refs establish generic indexed lifecycle tracking for acknowledged targets; the acknowledged watch baseline identifies the exact component incarnation a ref publishes. Shared target users track independently, and removing one user cannot remove another's tracking.

Finite dirty-target cohorts share byte-paged SDK watches on the existing fixed Client/session. Last-user removals coalesce by original watch and exact member generations; they do not allocate a group per ref or scan unrelated groups. Removed refs fence locally before awaiting membership removal. A partial removal settles its confirmed targets and retains only unconfirmed member IDs for cleanup retry, using the public cross-generated-runtime error guard. The finite authoring queue settles earlier batches, then awaits the captured removals before opening its next batch, bounding self-generated observer traffic without waiting inside its own open batch. Rejected removals retain their exact cleanup journal and report failure, but do not skip acknowledged declaration cleanup. Ended tracking forbids new ref publication, not ordinary Core mutation or cleanup. Session loss remains an incomplete-cleanup result unless independently confirmed; no observer error implies successful Core cleanup. Standalone unmount publishes its authoring fence and shared attempt promise before calling any ref disposer; a reentrant unmount joins that attempt, and a later retry after rejection performs retained cleanup only.

Matching lifecycle observations invalidate only indexed bindings and schedule a finite dirty-binding publication on the existing commit queue. A replaced component incarnation never retargets an old handle, and delayed observations of an earlier incarnation do not terminalize a newer handle. Ended tracking, such as a closed connection, permanently fences that root's control refs and rejects later publication, including unchanged renders; a resolved promise from ended tracking is never reused. Recovery requires a fresh client/session and root with live acknowledged tracking. Other healthy connections/scopes are unaffected. Ref callback changes alone issue no structural writes; callback return disposers, null fallback and synchronous reentrancy follow exact assignment lifetime. [Tracking](src/reconciler/lifecycle-tracking.ts) and [ref publication](src/gui/control-ref-registry.ts) own the indexed bookkeeping.

Unskinned controls paint no implicit parts. Choose a complete client skin, or author individual `Theme`/`Skin` rows with generated `GuiTheme.encodeParts`/`GuiSkin.encodeParts` and `guiPaintPartIndex`. A row explicitly enables its part; per-control override properties win over shared theme properties. `Theme.em` scales paint lengths against the inherited font, and `Metrics.em` independently scales shared measurement lengths; zero keeps lengths absolute. Per-control metrics and part overrides are absolute. A `ThemeMotion` companion authors native Host-clock transitions; absent timing is immediate. References accept an exact handle or a declared entity’s unique symbolic name. Row-addressed writes refer to slots in the exact component incarnation; `null` clears an optional row property.

Control refs and value callbacks additionally require selected `ipp.lifecycle-publisher`; custom selected-System lists must include it. Rendering controls without refs or value callbacks needs no lifecycle subscription.

Raw `source={assetRef(id)}` and `assetField(GuiSkin.partsOffset(slot, "asset"), assetRef(id))` use the same immutable asset preparation/ready-replacement lifecycle as other components. No private row encoder or duplicated asset owner exists.

Momentary callbacks (`onPress`, `onSubmit`, a numeric TextInput's `onReject` for text that does not parse and `onDiscard` for an edit that ended without a commit, such as by Escape, and `onContextMenu` on every control) use the fixed session's acknowledged `subscribeGuiEffects` stream, never action replies. Feedback callbacks on every control, `onFocusChange`, which names the focused part such as a range's thumb or a Color's field or rail, and `onInteractionChange` (one pointer's hover, press and capture), come from a second subscription of the feedback class, opened only while some control declares one; they reach only the target control and never propagate to action listeners. Value callbacks (`onToggle`, a Button's `onSelectedChange`, `onScalarCommit`, which carries a range's `upper` beside its `value` in one event and a numeric TextInput's committed number, a Color's `onColorCommit`, which carries its four channels in one event, `onTextCommit`, `onScroll`, `onRangeChange`, and a Behavior's `onVisibleChange`) subscribe the watched fields while registered and fire on field observations, whoever wrote the value. Ordinary Entity declarations accept `onActionCapture` and `onAction`. [Callback types](src/gui/callbacks.ts) define what each callback receives. The matching control callback runs first. Momentary effects then capture root-first and bubble target-first along their immutable effect ancestry, not current JSX or current Core links; value callbacks bubble through React's own element tree. Exceptions report through root `onError`; `stopPropagation()` affects only subsequent JavaScript propagation, never committed state. Retained declarations use current callbacks, and callback-only renders send no structural writes.

Callback targets share the indexed lifecycle tracking used by control refs. Initial render readiness includes target publication and the observer subscription ACK; this is not an atomic declaration/observation transaction and pre-registration history is not replayed. Effects arriving during later author acknowledgements occupy the existing finite commit queue, so newly acknowledged targets can be resolved without a second event history or a current-tree ancestry fallback. Removal and terminal session/observer failure fence callbacks locally, including queued delivery. Observer cleanup retains its exact subscription until unsubscribe acknowledges; it does not skip ordinary declaration cleanup on failure or close a shared client.

Text submission is an explicit semantic submit action, validated against the control's identity and eligibility. The ordinary local System publishes a momentary effect carrying the submitted text without changing the value or moving focus; React's `onSubmit` observes that effect, not a later read. Physical Enter uses the selected input context and the same application observation channel. The former node protocol and GuiRoot wrapper remain removed. [Real callback scenarios](../../tests/react/gui-authoring/scenarios/gui-callbacks.tsx) port the ordinary application callback intent from historical `packages/ipp-react/tests/gui-effects.test.ts` and `gui-controls.test.ts` at base `e729a23da697fafdd30bc78af27441342ae4775d`; the historical current-tree fallback is deliberately not retained.

`IppCanvas` owns a physical input context alongside its explicitly selected presentation. The reusable [browser adapter](src/gui/platform/input.ts) relays pointer/key/wheel events, with the Shift state of keys and wheel, and a hidden native text buffer, converting each browser wheel notch to `guiInput.wheelStep` Canvas logical units (`DEFAULT_GUI_WHEEL_STEP` by default); it never owns committed text or dispatches application callbacks from routing replies. Pending native edits serialize against their actual predecessor ACK, while external focus/value changes cancel unsent edits and stale native composition. Delayed clipboard reads retain their original exact fence. `guiInput.unhandledInputGate` admits scene gestures only after their correlated routing outcome; capture loss, physical selection changes and detach fence old gestures. Soft-keyboard requests require trusted activation and available platform support. Secondary presses, the Menu key and Shift+F10 are runtime context requests, so the adapter suppresses the browser's own context menu over the canvas and its text buffer.

`Group` attaches a `GuiGroup` to the current Entity. The controls below it, down to any nested group or item, are its items: arrow keys along its axis, Home and End move among them, a group whose items take focus is one Tab stop, and one whose items do not has an active item that the `guiActiveItems` query reads. Selection stays in the items' `selected` fields, which the runtime writes when an item is activated in a group that selects; declare `selected` from application state and follow the runtime's writes like any other field. Tabs, trees, radio groups, menus and option lists are kit compositions over it.

`Overlay` attaches a `GuiOverlay`: its entity leaves the parent's flow, is placed against the parent's box, or against the canvas at top level, and creates its own ordered scope above its owner, even with an omitted or zero `Style.layer`. Its independent `band` selects popup, dialog or notification priority. It is open while its `Behavior` is visible; declare that from application state and follow the runtime's writes of it through the Behavior's `onVisibleChange`, because its `mode` lets the runtime write it too. Manual (0) changes only by your writes. Light (1) closes on a press outside it and its parent, which it swallows, and when focus leaves both. Modal (2) blocks what lies beneath it in its canvas and keeps Tab inside. Hint (3) opens after a delay while its parent control is hovered or visibly focused and closes after a grace, so declare it closed. Opening a light or modal overlay moves focus to its first focusable control and closing it returns focus, and Escape closes the topmost overlay that is not manual. Dropdowns, menus, popovers, dialogs, tooltips and toasts are kit compositions over it.

### Ordinary scrolling

[`ScrollView` and `VirtualList`](src/gui/scroll.tsx) attach ordinary control components to the current Entity. `Children` still explicitly declares parenting; a ScrollView measures its single content child, while VirtualList realizes the wanted range stored in its fields through keyed ordinary entities with explicit `VirtualItem` indices. Every item stays in the containing World and uses the generic acknowledged declaration/cleanup path. `renderItem` declares the item's components and optional explicit children, not another GUI node tree.

Control handles issue scroll-to, scroll-by and scroll-to-index actions. `onScroll` and `onRangeChange` observe the control's offset and range fields, which a field subscription reports first as the current value and then as they change; no frame polling or action-reply callback synthesis is involved. Restored controls retain their stored position. Shrink/regrow and measurement anchoring follow the [GUI contract](../../docs/architecture/gui.md#layout-and-presentation).

[Real scrolling scenarios](../../tests/react/gui-authoring/scenarios/gui-scroll.tsx) and [completed paint assertions](../../tests/react/gui-authoring/pages/gui-paint.tsx) replace the ordinary realization/anchoring intent of historical `gui-virtual-list.test.ts` at the base above. The maintained `gui:physical-webgl` and `gui:physical-gles` scenarios exercise the reusable input adapter against real generated clients, composed child Worlds, committed effects and captures, including drag-versus-tap and native text. Synthetic composition/clipboard events and browser focus establish adapter behavior, not physical OS IME, hardware touch or soft-keyboard device coverage.

## GUI kit

Kit widgets own ordinary entities and native controls. Their common layout props use World logical units; spacing props also accept skin-scaled roles such as `gap="compact"`, `gap="field"` and `padding="inset"`. Explicit raw `layout` fields take precedence. See the shared [layout interface](src/gui-kit/layout.tsx) for sizing and alignment, and [spacing roles](src/gui-kit/spacing.ts) for the token vocabulary.

`Row` and `Column` fit their content height and supply compact and field spacing respectively. Row centers its children without imposing a control height; Toolbar adds control-row sizing and label spacing. ActionRow places its spaced controls at the trailing edge; its `leading` slot holds status or descriptive content at the opposite edge. PanelFooter uses the same slots with compact vertical padding. OverlayStack overlaps children in one box. Explicit dimensions, flex, zero gaps and `fitWidth`/`fitHeight` express intentional sizing and connected surfaces.

Panel is an unpadded frame. PanelHeader, PanelBody, PanelFooter and ScrollArea own their content insets; use `padding="inset"` on a Panel holding direct content. Fields fill available form width unless explicitly sized. Form, Field and FormSection compose labels, descriptions, errors and groups with shared presentation; TextField uses that same Field presentation. Dialog sections accept the shared layout props, and its actions use trailing placement. Popover's `contentWidth` sizes its floating content independently of the trigger's root `width`. Explicit dialog and popover widths and DataGrid column widths use logical units; only omitted defaults scale with the skin. TextField's `gap` controls spacing between its label, input, description and error; raw `layout.gap` takes precedence. Raw component declarations retain their last authored values when props are omitted; kit layout roots explicitly restore their own defaults.

[`ScrollArea`](src/gui-kit/containers/scroll-area.tsx) fits short content and scrolls within available space or a `maxHeight`. It supplies content padding, form spacing and scrollbar clearance, hides bars when content fits, and shares an enclosing Panel's surface. Use `padding={0}` for flush content or `fitHeight={false}` for an intentional fill; ordinary ScrollView remains the low-level declaration.

```tsx
<Panel width={300} maxHeight={480}>
  <ScrollArea>
    <TextField label="Name" />
    <Switch label="Enabled" />
  </ScrollArea>
</Panel>
```

```tsx
import { useState } from "react";
import {
  Panel,
  PanelBody,
  Form,
  ActionRow,
  Button,
  Switch,
  TextField,
} from "@ipp/react/gui-kit";

// Render inside a SkinProvider with its receiving contract, skin and font.
function Preferences() {
  const [enabled, setEnabled] = useState(false);
  const [name, setName] = useState("");
  return (
    <Panel width={280}>
      <PanelBody>
        <Form>
          <TextField label="Name" value={name} onChange={setName} />
          <Switch checked={enabled} onCheckedChange={setEnabled}>
            Sync
          </Switch>
        </Form>
        <ActionRow>
          <Button disabled={!name}>Save</Button>
        </ActionRow>
      </PanelBody>
    </Panel>
  );
}
```

Supplied `value`, `checked` and `open` props express application intent. The runtime commits input immediately; after the callback and React update, guarded writes restore edits the application declined. This is asynchronous convergence. Adapter writes use the current observed handle and compare-and-set against the reported value; the wire operation does not atomically guard a component incarnation. Defaults seed uncontrolled widgets once. Kit callbacks omit subscription baselines and their own write echoes; raw observation callbacks expose the baseline with `initial: true`. Autocomplete separates `inputValue`/`defaultInputValue` from selection. `Popover.Trigger`/`Content` and the `Dialog` sections compose custom content around native triggers and overlay behavior.

`@ipp/react/gui-kit` holds reusable compositions of the ordinary GUI declarations, such as panels with window controls, data grids, toasts, inline alerts, status badges, progress bars, spinners and expanders, and the parts they share: separators, secondary buttons, text lines, icon glyphs and the check mark. They consume an explicit skin, declare only ordinary entities, components, links and Host-clock animation, keep only application data of their own and leave interaction to the runtime's controls, as the [GUI client boundary](../../docs/architecture/gui.md#client-and-persistence-boundaries) requires. [The entry point](src/gui-kit.ts) lists them; each component's source documents its look.

Declare a `SkinProvider` or `GuiKit` in each World that uses the kit. It resolves the receiving contract from the root, or accepts an explicit matching contract, and takes the shared GUI font and body text size in that World's units. Providers may appear inside `Children`; their shared resources stay outside the native layout tree. The first `GuiKit` of a World declares the kit's themes there once, as ordinary theme entities that kit components reference; a `GuiKit` nested in the same World inherits that skin unless it declares its own skin and distinct `id`, while an `AttachedWorld` or `CanvasWorld` needs its own. Kit components refuse to render without one in their World.

`reducedMotion` on `GuiKit` is the application's one reduced-motion setting. The `GuiKit` that declares a World's themes sends it to that World as the GUI preference, which snaps the runtime's skin transitions, whenever it changes, and like the themes the preference goes with that kit: removing the kit or the setting from a live root turns off a preference it turned on, while unmounting the root leaves it. Every kit animation reads the setting of its nearest `GuiKit`, so spinners and rings stand still, leading sections stay steady and toasts drop without their fade; a nested `GuiKit reducedMotion` holds the kit's motion beneath it still without sending anything, and a `GuiKit` in an attached World inherits the setting and sends it to its own World. Omitted everywhere, the kit sends nothing. The World's preference reads back through the generated client's `guiPreferences` inspection (`client.inspectPage({ collection: "guiPreferences" })`), whoever set it.

The kit consumes the selected skin’s composite tokens, theme recipes and control mappings. `cybertronSkin(contract)` from `@ipp/react/gui-cybertron` recreates the original appearance; `glassSkin(contract, options)` from `@ipp/react/gui-glass` supplies its own complete palette, parts and shared shader. Pass either to `<GuiKit id="app-skin" skin={skin} contract={contract} font={font} fontSize={16}>`. Keep the provider and its `id` stable when switching skins. Use `<SkinProvider>` from `@ipp/react/gui` for the shared native and kit context; `GuiKit` also provides it. Native controls select their own roles automatically, including dial sliders and virtual-list scroll chrome. Use `<Checkbox skin="switch" />` to choose an alternate role or `skin={false}` to disable automatic appearance on a control. An explicit sibling `<Skin>` replaces the automatic skin regardless of declaration order; removing it restores inheritance. Controls without a provider remain unstyled, and context never crosses a World boundary. Multiple providers with distinct IDs can use different skins in one World. `GuiSkinDefinition` also accepts application-owned recipes and assets, without extending the runtime or generated contract. A kit theme missing from an explicit appearance produces an authoring error rather than falling back. Asset-backed definitions declare `assetIds` and an `assets` function whose stable identity identifies an immutable resource bundle. Replacement bundles load before visible theme bindings switch; a failed bundle leaves the prior ready skin selected.

Composites are assembled from parts rather than configured: a panel is a column of a `PanelHeader` holding its `WindowControls`, the application's body and separators, and a `PanelFooter`, and minimised it is the same frame unlit with only its header. Interaction stays with the runtime's controls: data grid rows, toast bodies and window controls are Buttons, and the application owns what they change, such as the selected row, the focused or editing cell and the list of toasts, receiving keys through callbacks. A `ToastStack` is a top-level manual overlay declared outside any `Children`; a toast that dismisses itself counts down with an animation controller on the Host clock, paused through its controls' feedback callbacks while it is hovered or focused, and never with a client timer.

The choice composites, `RadioGroup`, `SegmentedControl`, `Tabs` and `TreeView`, hold Button items in a `Group`, so the runtime gives them their keys, one Tab stop and immediate selection, which lives in the items' `selected` fields: arrows select in a radio group or segmented control, while tabs and tree rows select when activated. Each reports the item the runtime selected through `onChange`, from the items' `onSelectedChange`, and takes `value` or `defaultValue`. An item seeds its native selection at mount. Controlled selections converge to the latest supplied `value` after callbacks and React updates, using guarded writes through the current item handles. A tab strip that overflows scrolls between docked scroll buttons, and with `overflowMenu` adds a More menu of its tabs. A tree's rows are declared only while their branches are expanded; a vertical group has no use for Right and Left, so the runtime returns them unhandled to the client that sent them, and the application passes them to the tree's handle (`ref.current.key(key)`, from `guiInput.onUnhandled`), which expands, collapses and moves focus with a client focus action.

The built-in skins share [compact sizing](src/gui-kit/sizing.ts), so changing appearance preserves field, section and control geometry at a fixed body size. Keep form spacing in the kit's label, field and section roles, use ordinary controls for full interaction rows, and reserve smaller visible marks for choices. Prefer `fontSize` to scale a whole interface; use explicit layout dimensions for deliberate exceptions rather than compensating for a particular skin.

Menus, popovers, tooltips and dialogs are floating surfaces over the runtime's overlays, sharing [overlay.tsx](src/gui-kit/overlays/overlay.tsx): `GUI_KIT_OVERLAY_BANDS` names popup, dialog and notification priorities independently of interaction mode. Whole scopes retain their nested overlays, so a later dialog stays above an earlier dialog and its menus. Each public kit component accepts a relative `layer` offset applied once at its root; its labels and decorations inherit it. Occupied groups compact into physical ranks for Surface spacing; `useOverlayOpen` keeps an application's open state in step with the runtime, which closes a light or modal overlay itself, through the overlay Behavior's `onVisibleChange`, adopting a closing only from the entity it saw open, so an overlay nested in it never closes it; and `Floating` is the surface. A `Menu` holds command rows that take no focus in a group, so focus stays on the control that opened it while hover and the arrows move its active row, and it reports a command once for each opening. `ContextMenu` opens a menu at the point of a target's context request, from `useContextMenu`'s `opener`, and is a canvas root; a `Popover` is a light overlay below its own trigger holding the application's content; a `Tooltip` is a hint declared closed inside its control's `Children`; and a `ConfirmationDialog` is a modal canvas root whose Cancel takes focus when it opens and which reports its answer once. Their close buttons take no focus, Escape being their key, and the runtime returns focus to the control that opened them.

The selection controls—`Dropdown`, `SearchableDropdown`, `MultiSelect` and `Autocomplete`—extend their trigger or field into one continuous surface in every skin. Native placement can open the list above or below; the runtime retains keyboard navigation, focus and dismissal. Applications own options, suggestions and selected values. The [selection components](src/gui-kit/selection) document their individual callbacks and composition.

The value composites, `LabelledSlider`, `RangeSlider` and `Knob`, put the runtime's slider, presented along a horizontal or vertical rail or as a dial, among its caption, the readouts of its values with their `units` and the captions of its range's ends; a `SliderScale` labels a rail with ticks, and a `NumericStepper` puts the runtime's numeric text input with its step parts between a caption with the range's ends and its unit label. The runtime owns the values and their keys, drags, entries and focus, a range's thumbs being two focus stops; the composite reports each committed change through `onChange`, a range's two values together, and its readouts follow that commit without a tween. The control seeds its native values at mount. Controlled values converge through compare-and-set after callbacks and React updates; range fields are ordered to keep their bounds valid. Scalar `onChange` handlers may return a `Promise<void>` that settles after asynchronous acceptance and its parent render. Later native drag samples continue immediately; the readout follows native observations while acceptance is pending, and an unchanged prop restores a refused edit after settlement. Return the update promise from wrappers rather than detaching it. Marks, scale ticks and readouts that follow a thumb sit at the runtime's value-to-position mapping, so they line up with the thumb at any rail length; a range's readouts hang from their thumbs away from each other, so they never overlap. `format` gives a value's text, by default the step's decimals with positive values signed on a range below zero. A stepper shows an entry the runtime rejects as an error alert below its field until a later commit, submission or blur, or until Escape discards the edit. Valid steppers fit their caption and field; an alert expands the content below without moving the field. Use `reserveErrorSpace` when stable validation room is required. A `Knob`'s `children` go under its housing: its paired input is a `NumericStepper` without step parts that shares the knob's value, so typing turns the knob and turning it updates the field. A `ColorPicker` puts the runtime's colour control among its caption, the names of its rails, R, G and B byte fields and an A field in percent, preset swatches and a hex entry stated as sRGB. Its one colour is the control's HSVA, `onChange`'s value; the hex (`#RRGGBB`, or `#RRGGBBAA` with coverage alpha as a byte), the channels and the presets convert to and from it with the exported `formatHex`, `parseHex`, `hsvaToRgba` and `rgbaToHsva`, which keep the hue of a grey or black, and an entry sets the control with its value action, so the runtime reports it like a drag. A hex that does not parse shows an error in the line the picker reserves at its bottom and leaves the colour unchanged.

The spinner and the circular progress ring draw the arc shape on skinned entities and fit their content rather than their container's width. A spinner, or a ring of unknown total, turns its lit arc through an ordinary looping animation of the arc's start that exists only while it turns, so its World selects `ipp.animation` and the application removes a spinner as soon as its operation ends.

A progress bar draws a reported fraction or a list of parts, each in a palette tone, side by side in one track. While a bar or ring of known total is running, a short leading section just ahead of its fill fades in and out through an ordinary looping animation of its opacity, so it never looks stopped; it holds still under the kit's `reducedMotion` and goes when the task has an outcome.

```tsx
import * as contract from "./generated.js"; // the runtime's generated client
import { assetRef } from "@ipp/react";
import { cybertronSkin } from "@ipp/react/gui-cybertron";
import {
  DataGrid,
  GuiKit,
  InlineAlert,
  Panel,
  PanelHeader,
  ProgressBar,
  ToastStack,
  WindowControls,
} from "@ipp/react/gui-kit";

<GuiKit skin={cybertronSkin(contract)} contract={contract} font={assetRef("gui-font")} fontSize={16}>
  <Panel id="nodes" layout={{ width: 480, height: 360 }}>
    <PanelHeader id="nodes/header" title="NODE STATUS">
      <WindowControls id="nodes/controls" onClose={close} />
    </PanelHeader>
    <InlineAlert
      id="nodes/lost"
      severity="warning"
      text="Connection lost."
      action={{ label: "Reconnect", onPress: reconnect }}
    />
    <DataGrid
      id="nodes/grid"
      columns={[
        { key: "node", title: "NODE" },
        { key: "signal", title: "SIGNAL", width: 96, align: "end" },
      ]}
      rows={nodes}
      selected={selected}
      onRowPress={select}
      layout={{ flex: 1 }}
    />
    <ProgressBar id="nodes/upload" label="Uploading" value={progress} />
  </Panel>
  <ToastStack id="toasts" toasts={toasts} onDismiss={dismiss} />
</GuiKit>;
```

## Resources and animation

The `canvasWorldOptions`, `guiWorldOptions` and `sceneWorldOptions` presets include native lifecycle observations for React refs and field hooks. An explicit `selectedSystems` replaces the preset selection.

`Animation playing={...}` updates native playback intent when the prop changes; `autoPlay` is only the initial default. Imperative controls remain available and the Host owns all playback clocks. `Transform` accepts position, Euler rotation or quaternion tuples and scalar or tuple scale; mixing a tuple with its corresponding scalar fields is rejected.

[Asset declarations](src/assets/declarations.ts) have root-local IDs and immutable inputs. Replace data/clip identity when content changes; in-place mutation bypasses encoding caches. Named consumers retain their prior selection until a replacement is ready, including failed or superseded loads. Unmount releases producer ownership while preserving other consumers. Host-local resource references do not make saved Worlds portable.

`AnimationAsset` supplies a reusable clip; `Animation` owns a controller. Playback refs express intent while the Host owns time, including atomic signed-speed playback. Structural bindings use the typed `entityLink` target and clip-local `entityBindings`, whose entries accept runtime handles or unique scene Entity names and resolve after acknowledgement. An optional transition blends changed numeric, quaternion and pose bindings with an explicit duration, easing and destination-time policy; discrete and structural bindings reject transitions. Ready replacements preserve playback state; cleanup removes controllers before deleting their targets. [animation/declarations.ts](src/animation/declarations.ts) defines bindings and handles; the [lighting example](../../examples/demos/scenes/lighting/animation-controller.tsx) demonstrates playback.

## Custom material parameters

`ShaderAsset` owns immutable stages and explicit recipe/parameter requirements. `CustomMaterial` supplies independently editable typed values, so parameter changes do not recompile the shared shader. [Shader declarations](src/shaders.ts), [value helpers](../ipp-client/src/dynamic-properties.ts) and the [material implementation guide](../../crates/ipp-render-gl/src/services/render/materials/CUSTOM_MATERIALS.md) define that interface. External applications must configure their bundler for shader-text imports.

## Validation

Run `python tools/ipp.py regression --suite react-attached --suite canvas` for the maintained native/worker reconciliation and browser/GLES presentation cases. The [suite registry](../../tools/pipeline/suites.json) includes additional material, animation and completed-frame coverage.

`IppCanvas` applies `guiInput` updates to its active physical input owner. Equivalent blocker identities and callback-only changes preserve capture and native text focus; changing blocker lifetimes, wheel step or the unhandled-input gate closes and reopens the existing Host context, cancelling its previous interactions. Pending acquisitions are fenced across view changes and disposal. [The input adapter](src/gui/platform/input.ts) also exposes `CanvasGuiInput.update` for applications managing the owner directly.

The DOM-free [`@ipp/react/canvas`](src/canvas.ts) entry point provides Canvas authoring sessions and declaration scopes for shared browser and native applications. The generic `@ipp/react` entry point remains independent of Canvas session lifecycle and presentation.

Concrete `FlatSurface`, `CylinderSurface` and `SphereSurface` declarations store canonical extents, signed curvature and spacing. The client-owned radius and shape-specific angular recipes exported by this package translate ergonomic configurations into those fields; [surface-config.ts](../ipp-client/src/surface-config.ts) defines their units and validation.
