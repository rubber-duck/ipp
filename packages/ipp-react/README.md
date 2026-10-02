# React world components

`@ipp/react` turns committed React declarations into ordinary IPP entity, component and link writes. Applications compose props; the core owns lifetime, required components and evaluation. Definitions resolve against each receiving generated client without retaining session identities.

Build with `python tools/ipp.py build react` from the repository root. Import scene declarations from `@ipp/react` and browser composition from `@ipp/react/web`. React is a peer dependency; the client and reconciler are package dependencies. Applications supply React DOM. [package.json](package.json) owns versions and exports; [index.ts](src/index.ts) defines the public entry point.

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

This writes a Scalar on an existing entity named by its symbolic id. `Entity` requires `id` (create the entity, or adopt an existing entity with that symbolic id) or `bindTo` (reference an existing entity that the root neither creates nor deletes). A render may declare each `id` only once: two `<Entity id>` nodes with the same id reject the render locally with `ReactWorldDuplicateEntityError`, naming the id, before anything is sent. An id that moves to another node between renders (a keyed remount, or a declaration moved elsewhere in the tree) keeps its entity. Several `bindTo` nodes, including one beside the `<Entity id>` in the same render, may refer to one entity; a component that another of them still declares stays when one declaration of it is removed. A component's first commit inserts it with its declared fields, or writes those fields in place when it already exists; later renders write changed fields. The last write wins: removing a prop leaves its last value, and no earlier value is restored. Removing a declaration while the root is mounted deletes what it declared: a removed `<Entity id>` deletes its entity and a removed component node removes its component, whether the root created or adopted them; a removed `<Entity bindTo>` deletes nothing, though its removed component nodes still remove their components. Unmounting deletes nothing: content that must disappear with a client lives in a World the client creates (temporary if it should end with the connection), and an author who wants cleanup removes the nodes before unmounting. Parenting is explicit through `Children` or `EntityLink`; nesting alone grants neither parenting nor lifetime. [components.ts](src/components.ts) defines props; the [core lifetime contract](../../docs/architecture/runtime.md#entity-lifetime) governs references and deletion.

`Children` links its direct Entity declarations to the enclosing Entity in keyed sibling order. `EntityLink` instead declares `parent` and optional `before`, each a runtime handle or a unique scene Entity's `id`/`bindTo`; `parent={null}` declares a root and omitted/null `before` appends. These write non-owning core links, not components. Removing a link declaration does not restore an earlier relationship; deleting a parent the root created never deletes other children. Competing link declarations for one actual entity reject within a root after acknowledgement, while independent roots follow last write wins. `ParentJoint` selects a joint against the current parent independently of the link. The selected World manifest governs component and operation admission, not compiled descriptors alone.

Link commits order known parent declarations and `before` anchors before their dependants, including dependencies through unchanged declarations and references resolved by runtime handle. This supports reparenting and chain reversal within the root's declared topology without temporary detach operations. It does not inspect or mirror ancestry written by other clients, other roots or animation: those can still cause core rejection. These edits are ordered, not atomic; corrected renders and unmount retain the ordinary acknowledged cleanup contract.

`render()` acknowledges declarations; `flush()` captures a finite acknowledgement cutoff after the scheduled React commit. It seals pending render slots so later internal hook updates and producer renders cannot replace or extend the captured work. Already-started attachment preparation reserves a position in that record's existing ordered work, covering its initial portal declarations, nested initialization and attachment acknowledgement. Each cutoff retains that position alongside its captured local commits; unrelated parent acknowledgements neither cancel the boundary requirement nor add later work. Boundary generation/lifetime changes fence superseded initialization; retirement-delayed replacements remain outside the cutoff. Rapid renders otherwise replace superseded descriptions that have not started submission, and their promises settle with the description actually applied in that pending queue position. Explicit commands and cleanup preserve their order. Neither method waits for resource readiness or arbitrary application promises. Unmount roots before closing their client. Failed batches may retain partial effects: the applied prefix updates the root's records, and corrected declarations can recover, but unchanged rejected work is not retried. When the applied extent is unknown or a render fails, the root deletes what its records hold and recommits; symbolic ids reach entities whose creation is uncertain. Roots do not migrate between sessions, and general remote refs remain unsupported. A commit's cost follows what React changed: one that only replaces props describes and compares just the declarations React updated, while one that inserts, removes, hides or reveals declarations, or changes what other declarations resolve against, compares the whole tree. See [commit handling](src/commits.ts) and [description](src/tree.ts).

`unmount()` permanently fences authoring and refs and releases the root's subscriptions and sessions; it sends no deletes, removes no animation controller or asset and neither detaches nor destroys attached Worlds. Concurrent calls share the current cleanup attempt, and a completed successful attempt is reused. After rejection, calling `unmount()` again explicitly retries only retained cleanup; it does not render, recommit declarations or close the shared client. Each returned promise preserves its own outcome, including the original failure. Keep the root until cleanup succeeds or retain the appropriate composition recovery owner when exact remote effects remain unresolved.

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
    <Entity id="anchor"><Surface width={1} height={1} /></Entity>
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

The pinned reconciler's portal return declaration omits React's element fields; the local TypeScript conversion reflects the actual React portal implementation, not an alternate renderer or context bridge. [Attachment types](src/attached-world.ts), [coordinator](src/attachment-state.ts) and [real transport scenario](../../tests/react/attached-world-case.ts) own the API and lifetime details.

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

The selection must include `ipp.canvas`. `extent` and `unitsPerMetre` set the initial canvas state at creation; later changes send one Canvas state update on the World's session, without a reply, and an omitted prop keeps the current value. Invalid values are refused before anything is created. A changed `create` or presentation creates a new World. `presentation={{ anchor }}` presents the canvas as a SurfaceCanvas child of that parent Surface anchor, through the `AttachedWorld` machinery and its ownership rules; the parent root needs `createRoot(client, { host })`. `presentation={{ root: true }}` needs an ancestor `IppCanvas` (or a root from its handle), creates the World and its own session, and supplies the Canvas root output while `IppCanvas.output` is omitted; an explicit output or a second root CanvasWorld is reported as a conflict. Removing the element destroys its World; closing the Canvas or unmounting the root leaves it to the Host. The ref and `onReady` receive `{ world, output, closed }`, where `output` is `canvasOutput(world)`. [Source](src/canvas-world.ts) and the [real transport scenario](../../tests/react/canvas-world-case.ts) own the details.

### Browser composition

`IppCanvas` owns its worker Host and DOM surface. Supply `world={{ create: options }}` or `world={{ load: { url, options } }}` and an explicit `output`: a Camera output, `canvasOutput(world)` for a World that selects the Canvas System, or null; omit it when a root `CanvasWorld` supplies the root. Created Worlds, here and in `AttachedWorld`, name their `selectedSystems`; there is no default selection. Null presents nothing; there is no first-Camera or first-Canvas fallback. World creation options, including `temporary`, pass through unchanged. Graph loading retains every created World identity, including a failed loader's cleanup journal. Configure resource relocation and optional `assetCacheBytes` in `runtime` before startup; the Host keeps up to 64 MiB of unused completed assets for reuse by default, and `assetCacheBytes` overrides that target (0 evicts on release); `browserRuntime(baseUrl)` supplies distribution URLs.

Each nested `World` owns a declaration scope in the fixed authoring World. `World.onCommit(scope)` can call `scope.bindOutput(entity, "camera")` after acknowledgement; binding resolves that Camera's exact identity but does not select presentation. A World's canvas needs no binding: `canvasOutput(world)` names it. Pass the result to `IppCanvas.output`. Selection may name a different World without retargeting any authoring container. `AttachedWorld` retains its explicit portal/context and separate ownership. DOM and scene declarations still have separate React roots: bridge application providers explicitly, and use scene-compatible error/Suspense fallbacks.

`initialize(client, signal, host)` runs before scopes and `onReady`. `onReady` exposes authoring readiness, not a completed draw; `onViewChange` reports an acknowledged physical selection. Callback-only updates do not author components or rebind presentation. Runtime/create/load configuration replacement creates a new owner; output changes and CSS/DPR resizing keep the authoring sessions, and resizing an unchanged output resizes the selected view in one Host request, which keeps the renderer's retained state. Drawing dimensions are negotiated against the actual surface before root binding. `handle.viewport` is the acknowledged viewport, not the latest requested CSS extent.

`flush()` inserts a finite ordered acknowledgement barrier for work already queued by committed React DOM, including its scopes' acknowledgement work. Later renders cannot extend it, and attachment retirement runs outside that queue. A root returned by `handle.createRoot()` has its own ordered `flush` without reentering the Canvas queue. `frame(options)` and `capture(options)` follow that authored-work barrier with the actual Host presentation request, preserving explicit publication/sequence constraints. They do not wait for future renders or imply asset readiness. Captures retain immutable completed-frame identities and bytes; diagnostics are separate and never authorize a frame.

Context loss preserves authoring. Call `recoverPresentation()` explicitly after a fresh context is available: it verifies the exact current root generation and refuses a same-context stale selection or externally replaced root. A known-unsent/typed rejected selection can be explicitly retried with the same output and size, reusing an acknowledged root binding. If recovery acknowledges a resized root but selection rejects, that binding remains authoritative for exact cleanup and explicit selection or recovery retry on the already-acquired fresh context; a successful selection ends this retry permission. Generic Host write errors lack enough outcome information for safe replay and retain an unknown-effect journal instead. No read-after-write inspection substitutes for a root/view acknowledgement, and no cleanup restores a previous selection.

Teardown fences callbacks immediately, settles submitted work, conditionally clears only acknowledged root/view identities and unmounts its roots, which delete nothing, before closing owned sessions and its owned Host; it destroys no World itself. Borrowed Worlds and authoring sessions survive unless session ownership was explicitly granted to the controller. Generic terminal closure is not proof of World destruction or published retirement.

`CanvasCleanupError.recovery` (also reachable as `handle.cleanup`) retains the owner, worker/Host and exact journals after a failed teardown. `retry()` resumes cleanup without reauthoring. `abandon()` explicitly relinquishes cleanup, returns the journal, and may close an owned Host/worker; it does not claim that cleanup completed. It never closes a borrowed Host or destroys its Worlds. Retain this recovery object from the lifecycle error callback even after the DOM unmounts. The original `closed` promise records the first attempt; await the recovery operation for subsequent results. A failed declaration cleanup keeps the root journal available through the same owner.

`CanvasWorldSession({ host, client })` reuses these boundaries with an externally supplied Host and fixed authoring Client, borrowing both by default. [Controller source](src/canvas-world-session.ts), [presentation receipts](src/canvas-presentation.ts), [owner/recovery](src/canvas-lifetime.ts) and [real native/worker scenario](../../tests/render/canvas-controller.tsx) define the implementation. [DOM fixtures](../../tests/render/canvas-fixture.tsx) exercise both public package variants.

The former GUI relay is not wired into this entry point. The ordinary GUI/input port must preserve presented-path pointer/wheel/capture routing, keyboard and focus handoff, native IME/clipboard, trusted activation, cancellation and generation fences through the accepted input APIs; this presentation slice claims none of that input behavior. Gallery/query/navigation callers remain separate migrations.

## Ordinary GUI authoring

`@ipp/react/gui` supplies ordinary layout, group, overlay, style, box, paint, text/glyph/drawing/bitmap, font, behavior, theme/skin/motion and five control component declarations (Button, Checkbox, Slider, TextInput and Color), beside the scroll controls below. Put components inside `Entity`; use `Children` or `EntityLink` for layout/painter ancestry. There is no GUI root identity or separate reconciler. The receiving World must select the corresponding evaluators and the Canvas System, which makes it the canvas; `CanvasWorld` owns such a World. Configuration follows the generated field names in [the component contract](src/gui/manifest.ts).

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

Control values are ordinary fields: `selected` on a Button, `checked` on a Checkbox, `value` on a Slider, and `upper` too on a `range` Slider, `hue`, `saturation`, `value` and `alpha` on a Color, and `text` on a TextInput, or `value` on a `numeric` one. A declared value is written on first commit and whenever the prop changes, and the props of one component that change in one commit are written in one component write, validated together, so a range's two values never pass through an inverted state; on reconnect it overwrites the stored value, because the last write wins. There are no initial-value props. [Control handles](src/gui/control-ref.ts) expose the exact World/entity/component-incarnation `target`, `read()` of the control component's current fields, `compareAndSet(field, expected, value)`, which resolves false without effect when the field holds another value, and `action(a)`, which submits the semantic action as a one-command `guiAction` batch and resolves with the batch outcome: refused actions fail it with `StaleTarget`, `Unavailable`, `UnsupportedAction` or `InvalidValue` and change nothing. They are not feedback subscriptions. A ref publishes only after declaration acknowledgement and its lifecycle-watch baseline. Removing, hiding or replacing a declaration and terminal client closure fence its handles immediately; already submitted requests retain their protocol outcomes.

Control refs establish generic indexed lifecycle tracking for acknowledged targets; the acknowledged watch baseline identifies the exact component incarnation a ref publishes. Shared target users track independently, and removing one user cannot remove another's tracking.

Finite dirty-target cohorts share byte-paged SDK watches on the existing fixed Client/session. Last-user removals coalesce by original watch and exact member generations; they do not allocate a group per ref or scan unrelated groups. Removed refs fence locally before awaiting membership removal. A partial removal settles its confirmed targets and retains only unconfirmed member IDs for cleanup retry, using the public cross-generated-runtime error guard. The finite authoring queue settles earlier batches, then awaits the captured removals before opening its next batch, bounding self-generated observer traffic without waiting inside its own open batch. Rejected removals retain their exact cleanup journal and report failure, but do not skip acknowledged declaration cleanup. Ended tracking forbids new ref publication, not ordinary Core mutation or cleanup. Session loss remains an incomplete-cleanup result unless independently confirmed; no observer error implies successful Core cleanup. Standalone unmount publishes its authoring fence and shared attempt promise before calling any ref disposer; a reentrant unmount joins that attempt, and a later retry after rejection performs retained cleanup only.

Matching lifecycle observations invalidate only indexed bindings and schedule a finite dirty-binding publication on the existing commit queue. A replaced component incarnation never retargets an old handle, and delayed observations of an earlier incarnation do not terminalize a newer handle. Ended tracking, such as a closed connection, permanently fences that root's control refs and rejects later publication, including unchanged renders; a resolved promise from ended tracking is never reused. Recovery requires a fresh client/session and root with live acknowledged tracking. Other healthy connections/scopes are unaffected. Ref callback changes alone issue no structural writes; callback return disposers, null fallback and synchronous reentrancy follow exact assignment lifetime. [Tracking](src/control_tracking.ts) and [ref publication](src/control_refs.ts) own the indexed bookkeeping.

Controls without a theme paint their kind's built-in default look. Use generated `GuiTheme.encodeParts`/`GuiSkin.encodeParts` with monotone row slots and generated `guiPaintPartIndex`, not copied paint-key arithmetic; theme and override rows win over the default look property by property. A theme's `em` names the font size its lengths were designed at, so they follow each control's inherited font; zero keeps them absolute, and override rows are always absolute. Named built-in looks such as the switch for a checkbox come from the generated contract: `<Theme parts={GuiTheme.encodeParts(guiSkinLookTable("switch"))} em={GUI_SKIN_LOOKS.switch.em} />` declares an ordinary theme entity, and `<ThemeMotion parts={GuiThemeMotion.encodeParts(guiSkinLookMotionTable("switch"))} />` on the same entity gives it the look's transitions, which otherwise come from the control kind's default look. Component `fields` accepts generated sparse field writes such as `GuiSkin.patchPartsFields`; `null` clears an optional row property, while removing a prop leaves its last written value. References to theme entities accept an exact handle or one declared entity's unique symbolic name. Row-addressed writes refer to slots in the exact component incarnation.

Control refs and value callbacks additionally require selected `ipp.lifecycle-publisher`; custom selected-System lists must include it. Rendering controls without refs or value callbacks needs no lifecycle subscription.

Raw `source={assetRef(id)}` and `assetField(GuiSkin.partsOffset(slot, "asset"), assetRef(id))` use the same immutable asset preparation/ready-replacement lifecycle as other components. No private row encoder or duplicated asset owner exists.

Momentary callbacks (`onPress`, `onSubmit`, a numeric TextInput's `onReject` for text that does not parse and `onDiscard` for an edit that ended without a commit, such as by Escape, and `onContextMenu` on every control) use the fixed session's acknowledged `subscribeGuiEffects` stream, never action replies. Feedback callbacks on every control, `onFocusChange`, which names the focused part such as a range's thumb or a Color's field or rail, and `onInteractionChange` (one pointer's hover, press and capture), come from a second subscription of the feedback class, opened only while some control declares one; they reach only the target control and never propagate to action listeners. Value callbacks (`onToggle`, a Button's `onSelectedChange`, `onScalarCommit`, which carries a range's `upper` beside its `value` in one event and a numeric TextInput's committed number, a Color's `onColorCommit`, which carries its four channels in one event, `onTextCommit`, `onScroll`, `onRangeChange`, and a Behavior's `onVisibleChange`) subscribe the watched fields while registered and fire on field observations, whoever wrote the value. Ordinary Entity declarations accept `onActionCapture` and `onAction`. [Callback types](src/gui/callbacks.ts) define what each callback receives. The matching control callback runs first. Momentary effects then capture root-first and bubble target-first along their immutable effect ancestry, not current JSX or current Core links; value callbacks bubble through React's own element tree. Exceptions report through root `onError`; `stopPropagation()` affects only subsequent JavaScript propagation, never committed state. Retained declarations use current callbacks, and callback-only renders send no structural writes.

Callback targets share the indexed lifecycle tracking used by control refs. Initial render readiness includes target publication and the observer subscription ACK; this is not an atomic declaration/observation transaction and pre-registration history is not replayed. Effects arriving during later author acknowledgements occupy the existing finite commit queue, so newly acknowledged targets can be resolved without a second event history or a current-tree ancestry fallback. Removal and terminal session/observer failure fence callbacks locally, including queued delivery. Observer cleanup retains its exact subscription until unsubscribe acknowledges; it does not skip ordinary declaration cleanup on failure or close a shared client.

Text submission is an explicit semantic submit action, validated against the control's identity and eligibility. The ordinary local System publishes a momentary effect carrying the submitted text without changing the value or moving focus; React's `onSubmit` observes that effect, not a later read. Physical Enter uses the selected input context and the same application observation channel. The former node protocol and GuiRoot wrapper remain removed. [Real callback scenarios](../../tests/react/gui-callbacks.tsx) port the ordinary application callback intent from historical `packages/ipp-react/tests/gui-effects.test.ts` and `gui-controls.test.ts` at base `e729a23da697fafdd30bc78af27441342ae4775d`; the historical current-tree fallback is deliberately not retained.

`IppCanvas` owns a physical input context alongside its explicitly selected presentation. The reusable [browser adapter](src/gui/input.ts) relays pointer/key/wheel events, with the Shift state of keys and wheel, and a hidden native text buffer, converting each browser wheel notch to `guiInput.wheelStep` Canvas logical units (`DEFAULT_GUI_WHEEL_STEP` by default); it never owns committed text or dispatches application callbacks from routing replies. Pending native edits serialize against their actual predecessor ACK, while external focus/value changes cancel unsent edits and stale native composition. Delayed clipboard reads retain their original exact fence. `guiInput.unhandledInputGate` admits scene gestures only after their correlated routing outcome; capture loss, physical selection changes and detach fence old gestures. Soft-keyboard requests require trusted activation and available platform support. Secondary presses, the Menu key and Shift+F10 are runtime context requests, so the adapter suppresses the browser's own context menu over the canvas and its text buffer.

`Group` attaches a `GuiGroup` to the current Entity. The controls below it, down to any nested group or item, are its items: arrow keys along its axis, Home and End move among them, a group whose items take focus is one Tab stop, and one whose items do not has an active item that the `guiActiveItems` query reads. Selection stays in the items' `selected` fields, which the runtime writes when an item is activated in a group that selects; declare `selected` from application state and follow the runtime's writes like any other field. Tabs, trees, radio groups, menus and option lists are kit compositions over it.

`Overlay` attaches a `GuiOverlay`: its entity leaves the parent's flow, is placed against the parent's box, or against the canvas at top level, and needs a nonzero `Style` layer, which raises it above its parent. It is open while its `Behavior` is visible; declare that from application state and follow the runtime's writes of it through the Behavior's `onVisibleChange`, because its `mode` lets the runtime write it too. Manual (0) changes only by your writes. Light (1) closes on a press outside it and its parent, which it swallows, and when focus leaves both. Modal (2) blocks what lies beneath it in its canvas and keeps Tab inside. Hint (3) opens after a delay while its parent control is hovered or visibly focused and closes after a grace, so declare it closed. Opening a light or modal overlay moves focus to its first focusable control and closing it returns focus, and Escape closes the topmost overlay that is not manual. Dropdowns, menus, popovers, dialogs, tooltips and toasts are kit compositions over it.

### Ordinary scrolling

[`ScrollView` and `VirtualList`](src/gui/scroll.tsx) attach ordinary control components to the current Entity. `Children` still explicitly declares parenting; a ScrollView measures its single content child, while VirtualList realizes the wanted range stored in its fields through keyed ordinary entities with explicit `VirtualItem` indices. Every item stays in the containing World and uses the generic acknowledged declaration/cleanup path. `renderItem` declares the item's components and optional explicit children, not another GUI node tree.

Control handles issue scroll-to, scroll-by and scroll-to-index actions. `onScroll` and `onRangeChange` observe the control's offset and range fields, which a field subscription reports first as the current value and then as they change; no frame polling or action-reply callback synthesis is involved. Restored controls retain their stored position. Shrink/regrow and measurement anchoring follow the [GUI contract](../../docs/architecture/gui.md#layout-and-presentation).

[Real scrolling scenarios](../../tests/react/gui-scroll.tsx) and [completed paint assertions](../../tests/react/gui-paint.tsx) replace the ordinary realization/anchoring intent of historical `gui-virtual-list.test.ts` at the base above. The maintained `gui:physical-webgl` and `gui:physical-gles` scenarios exercise the reusable input adapter against real generated clients, composed child Worlds, committed effects and captures, including drag-versus-tap and native text. Synthetic composition/clipboard events and browser focus establish adapter behavior, not physical OS IME, hardware touch or soft-keyboard device coverage.

## GUI kit

`@ipp/react/gui-kit` holds reusable compositions of the ordinary GUI declarations, such as panels with window controls, data grids, toasts, inline alerts, status badges, progress bars, spinners and expanders, and the parts they share: separators, secondary buttons, text lines, icon glyphs and the check mark. They are drawn in the design language of the built-in looks, declare only ordinary entities, components, links and Host-clock animation, keep only application data of their own and leave interaction to the runtime's controls, as the [GUI client boundary](../../docs/architecture/gui.md#client-and-persistence-boundaries) requires. [The entry point](src/gui-kit.ts) lists them; each component's source documents its look.

Declare one `GuiKit` at the top of every World's declarations that uses the kit, outside any `Children`. It takes the receiving runtime's generated contract module, the shared GUI font and the body text size in that World's units. The first `GuiKit` of a World declares the kit's themes there once, as ordinary theme entities that kit components reference; a `GuiKit` nested in the same World declares nothing and changes only what it is given, such as the size of part of a panel, while an `AttachedWorld` or `CanvasWorld` needs its own. Kit components refuse to render without one in their World.

`reducedMotion` on `GuiKit` is the application's one reduced-motion setting. The `GuiKit` that declares a World's themes sends it to that World as the GUI preference, which snaps the runtime's skin transitions, whenever it changes, and like the themes the preference goes with that kit: removing the kit or the setting from a live root turns off a preference it turned on, while unmounting the root leaves it. Every kit animation reads the setting of its nearest `GuiKit`, so spinners and rings stand still, leading sections stay steady and toasts drop without their fade; a nested `GuiKit reducedMotion` holds the kit's motion beneath it still without sending anything, and a `GuiKit` in an attached World inherits the setting and sends it to its own World. Omitted everywhere, the kit sends nothing. The World's preference reads back through the generated client's `guiPreferences` inspection (`client.inspectPage({ collection: "guiPreferences" })`), whoever set it.

The kit keeps no colours or lengths of its own. Its themes are rows built from the contract's `GUI_SKIN_TOKENS` and built-in looks with the tokens' `em`, so their lengths follow each part's inherited font, and kit components scale their layout lengths by `fontSize / em` in the same way. Containers fill their container across, so kit components are fixed-height rows or columns that fill their container's width, except badges and buttons, which fit their labels, and the toast stack, an overlay of fixed width; `layout` places a component's root. An expander's content follows its header in the enclosing column, and a progress bar of unknown duration moves through an ordinary animation controller, so its World selects `ipp.animation`.

Composites are assembled from parts rather than configured: a panel is a column of a `PanelHeader` holding its `WindowControls`, the application's body and separators, and a `PanelFooter`, and minimised it is the same frame unlit with only its header. Interaction stays with the runtime's controls: data grid rows, toast bodies and window controls are Buttons, and the application owns what they change, such as the selected row, the focused or editing cell and the list of toasts, receiving keys through callbacks. A `ToastStack` is a top-level manual overlay declared outside any `Children`; a toast that dismisses itself counts down with an animation controller on the Host clock, paused through its controls' feedback callbacks while it is hovered or focused, and never with a client timer.

The choice composites, `RadioGroup`, `SegmentedControl`, `Tabs` and `TreeView`, hold Button items in a `Group`, so the runtime gives them their keys, one Tab stop and immediate selection, which lives in the items' `selected` fields: arrows select in a radio group or segmented control, while tabs and tree rows select when activated. Each reports the item the runtime selected through `onChange`, from the items' `onSelectedChange`, and takes `value` or `defaultValue`. An item declares its `selected` field only when it mounts, so a reported selection is never written back late over a newer one, and a `value` the runtime did not report is written through the item's handle. A tab strip that overflows scrolls between docked scroll buttons, and with `overflowMenu` adds a More menu of its tabs. A tree's rows are declared only while their branches are expanded; a vertical group has no use for Right and Left, so the runtime returns them unhandled to the client that sent them, and the application passes them to the tree's handle (`ref.current.key(key)`, from `guiInput.onUnhandled`), which expands, collapses and moves focus with a client focus action.

Menus, popovers, tooltips and dialogs are floating surfaces over the runtime's overlays, sharing [overlay.tsx](src/gui-kit/overlay.tsx): `GUI_KIT_LAYERS` names the canvas's layer planes, which every entity naming one shares wherever it is declared, one role each: content stays on plane 0, anchored overlays share plane 1, dialogs take plane 2, anchored overlays opened inside a dialog rise to plane 3, and toast stacks take plane 4, which deeper nesting also reaches; `useOverlayOpen` keeps an application's open state in step with the runtime, which closes a light or modal overlay itself, through the overlay Behavior's `onVisibleChange`, adopting a closing only from the entity it saw open, so an overlay nested in it never closes it; and `Floating` is the surface. A `Menu` holds command rows that take no focus in a group, so focus stays on the control that opened it while hover and the arrows move its active row, and it reports a command once for each opening. `ContextMenu` opens a menu at the point of a target's context request, from `useContextMenu`'s `opener`, and is a canvas root; a `Popover` is a light overlay below its own trigger holding the application's content; a `Tooltip` is a hint declared closed inside its control's `Children`; and a `ConfirmationDialog` is a modal canvas root whose Cancel takes focus when it opens and which reports its answer once. Their close buttons take no focus, Escape being their key, and the runtime returns focus to the control that opened them.

The selection controls, `Dropdown`, `SearchableDropdown`, `MultiSelect` and `Autocomplete`, open an `OptionList` on a light floating surface below their trigger or field. Its options are Buttons that take no focus in a group without selection, so focus stays on the trigger or field while the runtime's active item follows hover and the arrows, and Enter, or Space outside a text field, picks it; the runtime dismisses the list, moves focus into it and back, and swallows the outside press that closes it, while the component keeps only the open state its trigger toggles, through `useOverlayOpen`. A dropdown's pick closes the list and reports its key through `onChange` once; a searchable dropdown's field takes focus as the list opens and filters the options in the client, and Enter without an active option picks the first match; a multi-select's options toggle with a check mark while the list stays open; an autocomplete shows the application's `suggestions` for the text it reports through `onInputChange`, accepts the active suggestion with Enter (`onSelect`, writing its label to the field) and otherwise commits the typed text (`onCommit`). The application owns the options, the suggestions and the selection.

The value composites, `LabelledSlider`, `RangeSlider` and `Knob`, put the runtime's slider, presented along a horizontal or vertical rail or as a dial, among its caption, the readouts of its values with their `units` and the captions of its range's ends; a `SliderScale` labels a rail with ticks, and a `NumericStepper` puts the runtime's numeric text input with its step parts between a caption with the range's ends and its unit label. The runtime owns the values and their keys, drags, entries and focus, a range's thumbs being two focus stops; the composite reports each committed change through `onChange`, a range's two values together, and its readouts follow that commit without a tween. Like a choice's items, the control declares its values only when it mounts, and a `value` the runtime did not report is written through the control's handle by compare-and-set, a range's two fields one at a time so neither write inverts it. Marks, scale ticks and readouts that follow a thumb sit at the runtime's value-to-position mapping, so they line up with the thumb at any rail length; a range's readouts hang from their thumbs away from each other, so they never overlap. `format` gives a value's text, by default the step's decimals with positive values signed on a range below zero. A stepper shows an entry the runtime rejects as an error alert in the line it reserves under its field, so nothing moves, until a later commit, submission or blur, or until Escape discards the edit and the field shows the formatted number again. A `Knob`'s `children` go under its housing: its paired input is a `NumericStepper` without step parts that shares the knob's value, so typing turns the knob and turning it updates the field. A `ColorPicker` puts the runtime's colour control among its caption, the names of its rails, R, G and B byte fields and an A field in percent, preset swatches and a hex entry stated as sRGB. Its one colour is the control's HSVA, `onChange`'s value; the hex (`#RRGGBB`, or `#RRGGBBAA` with coverage alpha as a byte), the channels and the presets convert to and from it with the exported `formatHex`, `parseHex`, `hsvaToRgba` and `rgbaToHsva`, which keep the hue of a grey or black, and an entry sets the control with its value action, so the runtime reports it like a drag. A hex that does not parse shows an error in the line the picker reserves at its bottom and leaves the colour unchanged.

The spinner and the circular progress ring draw the arc shape on skinned entities and fit their content rather than their container's width. A spinner, or a ring of unknown total, turns its lit arc through an ordinary looping animation of the arc's start that exists only while it turns, so its World selects `ipp.animation` and the application removes a spinner as soon as its operation ends.

A progress bar draws a reported fraction or a list of parts, each in a palette tone, side by side in one track. While a bar or ring of known total is running, a short leading section just ahead of its fill fades in and out through an ordinary looping animation of its opacity, so it never looks stopped; it holds still under the kit's `reducedMotion` and goes when the task has an outcome.

```tsx
import * as contract from "./generated.js"; // the runtime's generated client
import { assetRef } from "@ipp/react";
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

<GuiKit contract={contract} font={assetRef("gui-font")} fontSize={16}>
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

[Asset declarations](src/assets.ts) have root-local IDs and immutable inputs. Replace data/clip identity when content changes; in-place mutation bypasses encoding caches. Named consumers retain their prior selection until a replacement is ready, including failed or superseded loads. Unmount releases producer ownership while preserving other consumers. Host-local resource references do not make saved Worlds portable.

`AnimationAsset` supplies a reusable clip; `Animation` owns a controller. Playback refs express intent while the Host owns time, including atomic signed-speed playback. Structural bindings use the typed `entityLink` target and clip-local `entityBindings`, whose entries accept runtime handles or unique scene Entity names and resolve after acknowledgement. An optional transition blends changed numeric, quaternion and pose bindings with an explicit duration, easing and destination-time policy; discrete and structural bindings reject transitions. Ready replacements preserve playback state; cleanup removes controllers before deleting their targets. [animation.ts](src/animation.ts) defines bindings and handles; the [lighting example](../../examples/world-gallery/worlds/lighting/animation-controller.tsx) demonstrates playback.

## Custom material parameters

`ShaderAsset` owns immutable stages and explicit recipe/parameter requirements. `CustomMaterial` supplies independently editable typed values, so parameter changes do not recompile the shared shader. [Shader declarations](src/shaders.ts), [value helpers](../ipp-client/src/dynamic-properties.ts) and the [material implementation guide](../../crates/ipp-render-gl/src/services/render/CUSTOM_MATERIALS.md) define that interface. External applications must configure their bundler for shader-text imports.

## Validation

Run `python tools/ipp.py regression --suite react-attached --suite canvas` for the maintained native/worker reconciliation and browser/GLES presentation cases. The [suite registry](../../tools/pipeline/suites.json) includes additional material, animation and completed-frame coverage.
