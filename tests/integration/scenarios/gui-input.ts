/** GUI pointer/keyboard/text input through a generated client, independent of process launch and wire layout. */
import type {
  GuiObservationBatch,
  GuiTextFence,
  GuiTextFocusState,
  WorldPersistenceHostClient,
} from "@ipp/client";
import {
  aliasId,
  cameraClient,
  componentFields,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";
import { type GuiTestClient, loadedFont } from "./gui-lifecycle.js";

function expect(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

/** Focused node of one panel as the semantic snapshot observes it. */
async function focusedNode(client: GuiTestClient, entity: bigint) {
  return (await client.semanticSnapshot({ entity })).focused?.id;
}

async function inspected(
  client: GuiTestClient,
  entity: bigint,
  nodeId: number,
) {
  const node = (await client.inspectGui({ entity, nodeId, maxDepth: 1 }))
    .nodes[0];
  expect(node, `GUI node ${nodeId} disappeared`);
  return node;
}

/** Every observation batch one client received, in delivery order. */
interface ObservationLog {
  readonly batches: GuiObservationBatch[];
  stop(): void;
}

function recordObservations(client: GuiTestClient): ObservationLog {
  const batches: GuiObservationBatch[] = [];
  const stop = client.subscribeGuiObservations((batch) => {
    batches.push(batch);
  });
  return { batches, stop };
}

/** Poll until `read` yields a value; observations trail routing replies. */
async function eventually<T>(
  read: () => T | undefined,
  message: string,
): Promise<T> {
  const deadline = Date.now() + 10000;
  for (;;) {
    const value = read();
    if (value !== undefined) return value;
    if (Date.now() > deadline) throw new Error(message);
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
}

/** The runtime's current text focus for this session, as last published. */
function currentTextFocus(log: ObservationLog): GuiTextFocusState | null {
  for (let index = log.batches.length - 1; index >= 0; index -= 1) {
    const state = log.batches[index]?.textFocus;
    if (state !== undefined) return state;
  }
  return null;
}

/** Wait until the current text focus satisfies `accept`. */
function focusWhere(
  log: ObservationLog,
  accept: (state: GuiTextFocusState | null) => boolean,
  message: string,
): Promise<GuiTextFocusState | null> {
  return eventually(() => {
    const state = currentTextFocus(log);
    return accept(state) ? { state } : undefined;
  }, message).then(({ state }) => state);
}

/** The fence a native buffer stamps after observing one focus state. */
function fenceOf(state: GuiTextFocusState): GuiTextFence {
  return {
    contextGeneration: state.contextGeneration,
    focusGeneration: state.focusGeneration,
    entity: state.entity,
    rootIncarnation: state.rootIncarnation,
    node: state.node,
    revision: state.revision,
  };
}

/** Text submissions published after batch `mark`. */
function submissionsSince(log: ObservationLog, mark: number) {
  return log.batches
    .slice(mark)
    .flatMap((batch) => batch.effects)
    .filter((effect) => effect.kind === "submitted");
}

/** Committed control effects published after batch `mark`. */
function commitsSince(log: ObservationLog, mark: number) {
  return log.batches
    .slice(mark)
    .flatMap((batch) => batch.effects)
    .filter((effect) => effect.kind === "controlCommitted");
}

/** Conflict reasons published after batch `mark`. */
function conflictsSince(log: ObservationLog, mark: number): string[] {
  return log.batches
    .slice(mark)
    .flatMap((batch) => batch.conflicts ?? [])
    .map((conflict) => conflict.reason.kind);
}

/**
 * Exercise real pointer/keyboard/text outcomes through the production
 * ingress: correlated `submitGuiInput` admissions against a live World,
 * observed through committed control values.
 *
 * The panel is a 4x3 Surface, so with the default units-per-metre the GUI
 * logical extent is exactly 4x3: the centre tap hits a full-panel
 * checkbox while a far point reaches no panel. Positions stay logical
 * units throughout; surface mapping helpers own the metre conversion.
 */
export async function exerciseGuiInput(
  host: WorldPersistenceHostClient<GuiTestClient>,
  fontBytes: ArrayBuffer,
) {
  const client = await host.createWorld({ symbolicId: "gui-input" });
  const log = recordObservations(client);
  const font = await client.createAsset(17, fontBytes);
  const ref = { kind: "alias", alias: 1 } as const;
  const entity = aliasId(
    await client.batch([
      createEntity(1, "gui-input-panel"),
      insertComponent(client, "Transform", ref),
      insertComponent(client, "Surface", ref, { width: 4, height: 3 }),
      insertComponent(client, "GuiRoot", ref),
    ]),
    1,
  );
  const empty = await client.inspectGui({ entity });
  expect(empty.nodes.length === 0, "A new GuiRoot must start empty");
  const rootIncarnation = empty.rootIncarnation;
  const handle = (id: number) =>
    client.createGuiNodeHandle(entity, rootIncarnation, id);

  await client.editGui({
    action: "insert",
    entity,
    rootIncarnation,
    id: 1,
    index: 0,
    data: { kind: "container", containerKind: "column" },
    style: { width: 4, height: 3 },
  });
  await client.editGui({
    action: "insert",
    entity,
    rootIncarnation,
    id: 2,
    parent: 1,
    index: 0,
    data: { kind: "checkbox" },
    values: { checked: false },
    style: { width: 4, height: 3 },
  });

  // A centre press completes on release over the same full-panel checkbox.
  const at: [number, number] = [2, 1.5];
  await client.submitGuiInput({
    kind: "pointerDown",
    pointer: 1,
    position: at,
    button: "primary",
  });
  await client.submitGuiInput({
    kind: "pointerMove",
    pointer: 1,
    position: at,
  });
  await client.submitGuiInput({
    kind: "pointerUp",
    pointer: 1,
    position: at,
    button: "primary",
  });
  let checkbox = await inspected(client, entity, 2);
  expect(
    checkbox.controlValue.kind === "bool" &&
      checkbox.controlValue.value === true,
    `Centre press missed the checkbox: ${JSON.stringify(checkbox.controlValue)}`,
  );

  // A press far outside the 4x3 logical extent reaches no panel: no toggle.
  const miss: [number, number] = [10, 10];
  await client.submitGuiInput({
    kind: "pointerDown",
    pointer: 2,
    position: miss,
    button: "primary",
  });
  await client.submitGuiInput({
    kind: "pointerUp",
    pointer: 2,
    position: miss,
    button: "primary",
  });
  checkbox = await inspected(client, entity, 2);
  expect(
    checkbox.controlValue.kind === "bool" &&
      checkbox.controlValue.value === true,
    "An off-panel press toggled the checkbox",
  );

  // Checkboxes commit on tap release, never on press: the down below
  // stages a provisional press only, so the later cancel and release change
  // nothing and the value holds through the hold.
  await client.submitGuiInput({
    kind: "pointerDown",
    pointer: 3,
    position: at,
    button: "primary",
  });
  await client.submitGuiInput({ kind: "pointerCancel", pointer: 3 });
  await client.submitGuiInput({
    kind: "pointerUp",
    pointer: 3,
    position: at,
    button: "primary",
  });
  checkbox = await inspected(client, entity, 2);
  expect(
    checkbox.controlValue.kind === "bool" &&
      checkbox.controlValue.value === true,
    `Cancel-after-down changed the provisional press: ${JSON.stringify(checkbox.controlValue)}`,
  );

  // A press released outside its control completes nothing.
  await client.submitGuiInput({
    kind: "pointerDown",
    pointer: 4,
    position: at,
    button: "primary",
  });
  await client.submitGuiInput({
    kind: "pointerUp",
    pointer: 4,
    position: miss,
    button: "primary",
  });
  checkbox = await inspected(client, entity, 2);
  expect(
    checkbox.controlValue.kind === "bool" &&
      checkbox.controlValue.value === true,
    `Release-outside committed the press: ${JSON.stringify(checkbox.controlValue)}`,
  );

  // An in-bounds tap commits exactly once.
  await client.submitGuiInput({
    kind: "pointerDown",
    pointer: 5,
    position: at,
    button: "primary",
  });
  await client.submitGuiInput({
    kind: "pointerUp",
    pointer: 5,
    position: at,
    button: "primary",
  });
  checkbox = await inspected(client, entity, 2);
  expect(
    checkbox.controlValue.kind === "bool" &&
      checkbox.controlValue.value === false,
    `In-bounds tap missed the checkbox: ${JSON.stringify(checkbox.controlValue)}`,
  );

  // Keys other than traversal admit without effect while nothing has focus.
  // Tab then enters the panel at its first control without a pointer, and
  // Enter toggles it back on; Escape and blur clear the focus. The taps
  // above focused the checkbox, so blur it first.
  await client.submitGuiInput({ kind: "blur" });
  const enterWithoutFocus = await client.submitGuiInput({
    kind: "key",
    key: "enter",
    pressed: true,
  });
  expect(
    enterWithoutFocus.unhandled?.kind === "noFocus",
    `Unfocused Enter was handled: ${JSON.stringify(enterWithoutFocus.unhandled)}`,
  );
  await client.submitGuiInput({ kind: "key", key: "tab", pressed: true });
  expect(
    (await focusedNode(client, entity)) === 2,
    "Tab without focus did not enter the checkbox",
  );
  await client.submitGuiInput({ kind: "key", key: "enter", pressed: true });
  checkbox = await inspected(client, entity, 2);
  expect(
    checkbox.controlValue.kind === "bool" &&
      checkbox.controlValue.value === true,
    "Focused Enter missed the checkbox",
  );
  await client.submitGuiInput({ kind: "key", key: "escape", pressed: true });
  await client.submitGuiInput({ kind: "blur" });
  await client.submitGuiInput({ kind: "text", text: "ignored without focus" });
  checkbox = await inspected(client, entity, 2);
  expect(
    checkbox.controlValue.kind === "bool" &&
      checkbox.controlValue.value === true,
    "Unfocused text changed the checkbox",
  );

  // Text input: focus, type, select, replace, compose, commit, then cancel.
  // Keep both controls inside the evaluated panel. Programmatic focus is
  // authority-fenced against the current layout, so a child laid out below a
  // full-height sibling is intentionally not eligible.
  await client.editGui({
    action: "update",
    handle: handle(2),
    patch: { style: { height: 1.5 } },
  });
  await client.editGui({
    action: "insert",
    entity,
    rootIncarnation,
    id: 3,
    parent: 1,
    index: 1,
    data: { kind: "textInput", text: "", placeholder: "" },
    style: { width: 4, height: 1.5, asset: font },
  });
  let field = await inspected(client, entity, 3);

  // Reverse traversal: Shift+Tab without focus enters the last control and
  // moves backward in tree order, wrapping at the start; Tab wraps forward.
  // The root column bounds traversal as a focus scope, which the semantic
  // tree reports; it holds every control, so the order is unchanged.
  await client.editGui({
    action: "update",
    handle: handle(1),
    patch: { style: { focusScope: true } },
  });
  const scoped = await client.semanticSnapshot({ entity });
  expect(
    scoped.nodes.find((node) => node.id === 1)?.focusScope === true &&
      scoped.nodes.find((node) => node.id === 2)?.focusScope === false,
    "The semantic tree omitted the focus scope",
  );
  const traversal: (number | undefined)[] = [];
  for (const key of ["backTab", "backTab", "backTab", "tab"] as const) {
    await client.submitGuiInput({ kind: "key", key, pressed: true });
    traversal.push(await focusedNode(client, entity));
  }
  expect(
    JSON.stringify(traversal) === JSON.stringify([3, 2, 3, 2]),
    `Keyboard traversal order is wrong: ${JSON.stringify(traversal)}`,
  );
  await client.submitGuiInput({ kind: "key", key: "escape", pressed: true });
  expect(
    (await focusedNode(client, entity)) === undefined,
    "Escape did not release keyboard focus",
  );

  await client.submitGuiInput({
    kind: "focus",
    handle: handle(3),
  });
  await client.submitGuiInput({ kind: "text", text: "Hi" });
  field = await inspected(client, entity, 3);
  expect(
    field.controlValue.kind === "text" && field.controlValue.value === "Hi",
    `Typed text missed the field: ${JSON.stringify(field.controlValue)}`,
  );
  await client.submitGuiInput({ kind: "setTextSelection", start: 1, end: 2 });
  await client.submitGuiInput({ kind: "text", text: "p" });
  field = await inspected(client, entity, 3);
  expect(
    field.controlValue.kind === "text" && field.controlValue.value === "Hp",
    `Selection replace missed: ${JSON.stringify(field.controlValue)}`,
  );
  await client.submitGuiInput({
    kind: "composition",
    text: "世界",
    caretStart: 6,
    caretEnd: 6,
  });
  await client.submitGuiInput({ kind: "commitComposition" });
  field = await inspected(client, entity, 3);
  expect(
    field.controlValue.kind === "text" && field.controlValue.value === "Hp世界",
    `Composition commit missed: ${JSON.stringify(field.controlValue)}`,
  );
  await client.submitGuiInput({
    kind: "composition",
    text: "??",
    caretStart: 2,
    caretEnd: 2,
  });
  await client.submitGuiInput({ kind: "cancelComposition" });
  field = await inspected(client, entity, 3);
  expect(
    field.controlValue.kind === "text" && field.controlValue.value === "Hp世界",
    `Cancelled composition committed: ${JSON.stringify(field.controlValue)}`,
  );

  // Transient caret, selection and provisional work never touches the
  // committed value or revision through the real pipeline: moves and
  // updates paint only. The mounted browser test asserts that paint in
  // completed frames; this scenario asserts the committed seams.
  const transientRevision = field.controlRevision;
  await client.submitGuiInput({ kind: "key", key: "left", pressed: true });
  await client.submitGuiInput({ kind: "setTextSelection", start: 1, end: 2 });
  await client.submitGuiInput({
    kind: "composition",
    text: "zz",
    caretStart: 2,
    caretEnd: 2,
  });
  field = await inspected(client, entity, 3);
  expect(
    field.controlValue.kind === "text" &&
      field.controlValue.value === "Hp世界" &&
      field.controlRevision === transientRevision,
    `Transient text state leaked into the commit: ${JSON.stringify(field.controlValue)}@${field.controlRevision}`,
  );
  await client.submitGuiInput({ kind: "cancelComposition" });

  // A provisional fenced to its focus never writes a new target: start
  // composition on the field, move focus to the checkbox, then commit.
  // The commit addresses the checkbox focus, so both values hold still.
  await client.submitGuiInput({
    kind: "composition",
    text: "stale",
    caretStart: 5,
    caretEnd: 5,
  });
  await client.submitGuiInput({
    kind: "focus",
    handle: handle(2),
  });
  await client.submitGuiInput({ kind: "commitComposition" });
  field = await inspected(client, entity, 3);
  expect(
    field.controlValue.kind === "text" && field.controlValue.value === "Hp世界",
    `Stale composition wrote across focus: ${JSON.stringify(field.controlValue)}`,
  );
  checkbox = await inspected(client, entity, 2);
  expect(
    checkbox.controlValue.kind === "bool" &&
      checkbox.controlValue.value === true,
    `Stale composition touched the new focus: ${JSON.stringify(checkbox.controlValue)}`,
  );
  await client.submitGuiInput({
    kind: "focus",
    handle: handle(3),
  });
  await client.submitGuiInput({ kind: "cancelComposition" });

  // Native buffers stamp edits with the focus and revision they observed.
  // A paste whose clipboard read started on the field but resolves after
  // focus moved to the checkbox conflicts and writes neither control.
  const fieldFocus = await focusWhere(
    log,
    (state) => state?.node === 3 && state.text === "Hp世界",
    "The focused field published no text focus",
  );
  const stalePaste = fenceOf(fieldFocus!);
  await client.submitGuiInput({ kind: "focus", handle: handle(2) });
  await focusWhere(
    log,
    (state) => state === null,
    "Moving focus to the checkbox never cleared the text focus",
  );
  let mark = log.batches.length;
  await client.submitGuiInput({
    kind: "text",
    text: "pasted",
    fence: stalePaste,
  });
  await eventually(
    () =>
      conflictsSince(log, mark).includes("focusMismatch") ? true : undefined,
    "A paste stamped before the focus move did not conflict",
  );
  field = await inspected(client, entity, 3);
  expect(
    field.controlValue.kind === "text" && field.controlValue.value === "Hp世界",
    `A stale paste wrote the old focus: ${JSON.stringify(field.controlValue)}`,
  );
  checkbox = await inspected(client, entity, 2);
  expect(
    checkbox.controlValue.kind === "bool" &&
      checkbox.controlValue.value === true,
    `A stale paste touched the new focus: ${JSON.stringify(checkbox.controlValue)}`,
  );

  // Stamps chain on the sender's own in-flight edits: two keystrokes sent
  // before the first one's observation both land.
  await client.submitGuiInput({ kind: "focus", handle: handle(3) });
  const typing = fenceOf(
    (await focusWhere(
      log,
      (state) => state?.node === 3,
      "Refocusing the field published no text focus",
    ))!,
  );
  await client.submitGuiInput({ kind: "key", key: "end", pressed: true });
  await Promise.all([
    client.submitGuiInput({ kind: "text", text: "!", fence: typing }),
    client.submitGuiInput({ kind: "text", text: "?", fence: typing }),
  ]);
  field = await inspected(client, entity, 3);
  expect(
    field.controlValue.kind === "text" &&
      field.controlValue.value === "Hp世界!?",
    `Chained stamped edits were lost: ${JSON.stringify(field.controlValue)}`,
  );

  // A stamped range after an equal-length external replacement conflicts
  // and the replacement republishes the focused text without new input.
  const beforeReplace = fenceOf(
    (await focusWhere(
      log,
      (state) => state?.node === 3 && state.text === "Hp世界!?",
      "The chained edits never reached the text focus",
    ))!,
  );
  field = await inspected(client, entity, 3);
  await client.editGui({
    action: "setControlValue",
    handle: handle(3),
    expectedRevision: field.controlRevision,
    value: { kind: "text", value: "Hp世" },
  });
  const replaced = (await focusWhere(
    log,
    (state) => state?.node === 3 && state.text === "Hp世",
    "An external replacement of the focused text did not refresh the bridge",
  ))!;
  expect(
    replaced.focusGeneration !== beforeReplace.focusGeneration,
    "An external replacement kept the focus generation",
  );
  mark = log.batches.length;
  await client.submitGuiInput({
    kind: "setTextSelection",
    start: 1,
    end: 2,
    fence: beforeReplace,
  });
  await client.submitGuiInput({
    kind: "text",
    text: "late",
    fence: beforeReplace,
  });
  await eventually(
    () =>
      conflictsSince(log, mark).filter((kind) => kind === "focusMismatch")
        .length === 2
        ? true
        : undefined,
    "Edits stamped before the replacement did not conflict",
  );
  field = await inspected(client, entity, 3);
  expect(
    field.controlValue.kind === "text" && field.controlValue.value === "Hp世",
    `Stamped edits rebased onto replaced text: ${JSON.stringify(field.controlValue)}`,
  );
  await client.submitGuiInput({
    kind: "text",
    text: "界",
    fence: fenceOf(replaced),
  });
  field = await inspected(client, entity, 3);
  expect(
    field.controlValue.kind === "text" && field.controlValue.value === "Hp世界",
    `An edit stamped against the refreshed text missed: ${JSON.stringify(field.controlValue)}`,
  );

  // Enter on the focused field submits the committed text exactly once,
  // with its revision and logical ancestor path; Enter during an open
  // composition belongs to the IME and submits nothing.
  field = await inspected(client, entity, 3);
  mark = log.batches.length;
  await client.submitGuiInput({ kind: "key", key: "enter", pressed: true });
  const submitted = await eventually(() => {
    const found = submissionsSince(log, mark);
    return found.length > 0 ? found : undefined;
  }, "Enter on the focused field published no submission");
  expect(
    submitted.length === 1 &&
      submitted[0]!.text === "Hp世界" &&
      submitted[0]!.revision === field.controlRevision &&
      JSON.stringify(submitted[0]!.path) === JSON.stringify([1, 3]),
    `Unexpected submission: ${JSON.stringify(submitted, (_, value) => (typeof value === "bigint" ? value.toString() : value))}`,
  );
  await client.submitGuiInput({
    kind: "composition",
    text: "zz",
    caretStart: 2,
    caretEnd: 2,
  });
  mark = log.batches.length;
  await client.submitGuiInput({ kind: "key", key: "enter", pressed: true });
  await client.submitGuiInput({ kind: "cancelComposition" });
  // One more ordered round trip lets any stray submission publish first.
  await client.submitGuiInput({ kind: "key", key: "enter", pressed: true });
  const afterComposition = await eventually(() => {
    const found = submissionsSince(log, mark);
    return found.length > 0 ? found : undefined;
  }, "Enter after the cancelled composition published no submission");
  expect(
    afterComposition.length === 1,
    `Enter during composition submitted: ${afterComposition.length} submissions`,
  );

  // A delayed native range is fenced to its revision: an equal-length
  // external replace ("Hp世界" is 8 bytes, like "Hpqrstuv") moves the
  // revision, so the stale range conflicts instead of rebasing and the
  // next insert lands at the reset end.
  field = await inspected(client, entity, 3);
  await client.editGui({
    action: "setControlValue",
    handle: handle(3),
    expectedRevision: field.controlRevision,
    value: { kind: "text", value: "Hpqrstuv" },
  });
  await client.submitGuiInput({ kind: "setTextSelection", start: 1, end: 2 });
  await client.submitGuiInput({ kind: "text", text: "!" });
  field = await inspected(client, entity, 3);
  expect(
    field.controlValue.kind === "text" &&
      field.controlValue.value === "Hpqrstuv!",
    `Stale selection rebased onto replaced text: ${JSON.stringify(field.controlValue)}`,
  );

  // A provisional fenced to its base revision never commits over an
  // equal-length external replace ("Hpqrstuv!" and "123456789" are both
  // 9 bytes): the commit conflicts instead, then a fresh provisional
  // commits exactly once.
  await client.submitGuiInput({
    kind: "composition",
    text: "zz",
    caretStart: 2,
    caretEnd: 2,
  });
  field = await inspected(client, entity, 3);
  await client.editGui({
    action: "setControlValue",
    handle: handle(3),
    expectedRevision: field.controlRevision,
    value: { kind: "text", value: "123456789" },
  });
  await client.submitGuiInput({ kind: "commitComposition" });
  field = await inspected(client, entity, 3);
  expect(
    field.controlValue.kind === "text" &&
      field.controlValue.value === "123456789",
    `Stale composition committed over replaced text: ${JSON.stringify(field.controlValue)}`,
  );
  await client.submitGuiInput({ kind: "cancelComposition" });
  await client.submitGuiInput({
    kind: "composition",
    text: "zz",
    caretStart: 2,
    caretEnd: 2,
  });
  await client.submitGuiInput({ kind: "commitComposition" });
  field = await inspected(client, entity, 3);
  expect(
    field.controlValue.kind === "text" &&
      field.controlValue.value === "123456789zz",
    `Composition commit missed or duplicated: ${JSON.stringify(field.controlValue)}`,
  );

  // Every committed control effect names its source. A user edit reports
  // "user"; an accepted external replacement of a checkbox, slider and text
  // input publishes the same committed effect marked "external" with the new
  // value and revision; a semantic action reports "semantic"; a stale
  // replacement still conflicts and publishes nothing.
  const userCommits = commitsSince(log, 0).filter(
    (effect) => effect.node === 3 && effect.source === "user",
  );
  expect(userCommits.length > 0, "Typed text published no user-sourced commit");
  await client.editGui({
    action: "insert",
    entity,
    rootIncarnation,
    id: 4,
    parent: 1,
    index: 2,
    data: { kind: "slider" },
    values: { value: 0.25, min: 0, max: 1, step: 0 },
    style: { width: 4, height: 0.5 },
  });
  const replacements = [
    { node: 2, value: { kind: "bool", value: false } },
    { node: 4, value: { kind: "scalar", value: 0.75 } },
    { node: 3, value: { kind: "text", value: "external" } },
  ] as const;
  for (const { node, value } of replacements) {
    const before = await inspected(client, entity, node);
    mark = log.batches.length;
    await client.editGui({
      action: "setControlValue",
      handle: handle(node),
      expectedRevision: before.controlRevision,
      value,
    });
    const published = await eventually(() => {
      const found = commitsSince(log, mark).filter(
        (effect) => effect.node === node,
      );
      return found.length > 0 ? found : undefined;
    }, `External replacement of node ${node} published no commit`);
    expect(
      published.length === 1 &&
        published[0]!.source === "external" &&
        published[0]!.revision === before.controlRevision + 1 &&
        JSON.stringify(published[0]!.value) === JSON.stringify(value),
      `Unexpected external commit for node ${node}: ${JSON.stringify(published, (_, item) => (typeof item === "bigint" ? item.toString() : item))}`,
    );
  }
  // The focused text refreshed from the replacement without further input.
  await focusWhere(
    log,
    (state) => state?.node === 3 && state.text === "external",
    "Replacing the focused text did not refresh the text focus",
  );
  const stale = await inspected(client, entity, 2);
  mark = log.batches.length;
  await client
    .editGui({
      action: "setControlValue",
      handle: handle(2),
      expectedRevision: stale.controlRevision - 1,
      value: { kind: "bool", value: true },
    })
    .then(
      () => {
        throw new Error("A stale external replacement was accepted");
      },
      () => undefined,
    );
  const semantic = await inspected(client, entity, 2);
  await client.semanticAction({
    entity,
    rootIncarnation,
    node: 2,
    expectedRevision: semantic.controlRevision,
    action: { kind: "toggle" },
  });
  const semanticCommits = await eventually(() => {
    const found = commitsSince(log, mark).filter((effect) => effect.node === 2);
    return found.length > 0 ? found : undefined;
  }, "A semantic toggle published no commit");
  expect(
    semanticCommits.length === 1 &&
      semanticCommits[0]!.source === "semantic" &&
      semanticCommits[0]!.revision === semantic.controlRevision + 1,
    `A stale replacement published or the semantic source was lost: ${semanticCommits.map((effect) => effect.source).join(",")}`,
  );

  // No ScrollView sits under the panel's controls: the wheel is reported
  // unhandled for scene controls instead of scrolling anything.
  const unscrolled = await client.submitGuiInput({
    kind: "scroll",
    position: at,
    delta: [0, -4],
  });
  expect(
    unscrolled.unhandled?.kind === "scrollUnconsumed",
    `A wheel over non-scrollable content was handled: ${JSON.stringify(unscrolled.unhandled)}`,
  );

  log.stop();
  await host.detachWorld();
  const scrolling = await exerciseGuiScrolling(host);
  const removal = await exerciseGuiRemoval(host, fontBytes);
  const keyboardEntry = await exerciseGuiKeyboardEntry(host);
  return {
    traversal,
    submissions: submitted.length,
    commitSources: [
      ...new Set(commitsSince(log, 0).map((effect) => effect.source)),
    ].sort(),
    unhandledScroll: unscrolled.unhandled?.kind,
    scrolling,
    removal,
    keyboardEntry,
  };
}

/**
 * Keyboard entry and cross-panel traversal follow the active camera in their
 * own World. Three 2x1 panels hold checkboxes 2 and 3 each: a back-facing
 * panel 2 m in front of the camera, created first, then front-facing panels
 * 10 m and 6 m away. Tab from no focus enters the nearest front-facing
 * panel, traversal crosses the front-facing panels by distance before the
 * back-facing one, and turning the camera around makes the formerly
 * back-facing panel the only front-facing entry.
 */
async function exerciseGuiKeyboardEntry(
  host: WorldPersistenceHostClient<GuiTestClient>,
) {
  const client = await host.createWorld({ symbolicId: "gui-keyboard-entry" });
  const placements = [
    { symbolicId: "gui-keyboard-back", z: 8, qy: 1, qw: 0 },
    { symbolicId: "gui-keyboard-far", z: 0 },
    { symbolicId: "gui-keyboard-near", z: 4 },
  ];
  const panels: bigint[] = [];
  for (const { symbolicId, ...placement } of placements) {
    const ref = { kind: "alias", alias: 1 } as const;
    const entity = aliasId(
      await client.batch([
        createEntity(1, symbolicId),
        insertComponent(client, "Transform", ref, placement),
        insertComponent(client, "Surface", ref, { width: 2, height: 1 }),
        insertComponent(client, "GuiRoot", ref),
      ]),
      1,
    );
    const { rootIncarnation } = await client.inspectGui({ entity });
    const checkbox = (id: number) =>
      ({
        action: "insert",
        entity,
        rootIncarnation,
        id,
        parent: 1,
        index: id - 2,
        data: { kind: "checkbox" },
        values: { checked: false },
        style: { width: 1, height: 1 },
      }) as const;
    await client.editGuiBatch([
      {
        action: "insert",
        entity,
        rootIncarnation,
        id: 1,
        index: 0,
        data: { kind: "container", containerKind: "row" },
        style: { width: 2, height: 1 },
      },
      checkbox(2),
      checkbox(3),
    ]);
    panels.push(entity);
  }
  const [back, far, near] = panels as [bigint, bigint, bigint];
  const name = new Map([
    [back, "back"],
    [far, "far"],
    [near, "near"],
  ]);

  // A perspective camera 10 m along +Z looks down -Z at every panel.
  const cameraRef = { kind: "alias", alias: 1 } as const;
  const camera = aliasId(
    await client.batch([
      createEntity(1, "gui-keyboard-camera"),
      insertComponent(client, "Transform", cameraRef, { z: 10 }),
      insertComponent(client, "Camera", cameraRef, {
        projection: 0,
        fov_y: Math.PI / 4,
        near: 0.1,
        far: 100,
      }),
    ]),
    1,
  );
  const cameras = cameraClient(client);
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => {
      stop();
      reject(new Error("The keyboard entry camera never activated"));
    }, 10000);
    const stop = cameras.onCameraStateChanged((event) => {
      if (event.changes.activeCamera !== camera) return;
      clearTimeout(timer);
      stop();
      resolve();
    });
    cameras.sendCommand({ type: "CameraActivateCommand", entity: camera });
  });

  /** Panel and node holding keyboard focus, from each semantic snapshot. */
  const focused = async () => {
    for (const entity of panels) {
      const id = await focusedNode(client, entity);
      if (id !== undefined) return `${name.get(entity)}:${id}`;
    }
    return "none";
  };
  const press = async (key: "tab" | "backTab" | "escape") => {
    await client.submitGuiInput({ kind: "key", key, pressed: true });
    return focused();
  };
  const enter = async (key: "tab" | "backTab") => {
    await press("escape");
    return press(key);
  };

  const entry = [await enter("tab"), await enter("backTab")];
  await enter("tab");
  const traversal: string[] = [];
  for (let step = 0; step < 6; step += 1) traversal.push(await press("tab"));
  expect(
    JSON.stringify(entry) === JSON.stringify(["near:2", "near:3"]) &&
      JSON.stringify(traversal) ===
        JSON.stringify([
          "near:3",
          "far:2",
          "far:3",
          "back:2",
          "back:3",
          "near:2",
        ]),
    `Keyboard entry ignored the view order: ${JSON.stringify({ entry, traversal })}`,
  );

  // Turning the camera around behind the panels leaves the formerly
  // back-facing panel as the only front-facing one.
  successfulBatch(
    await client.batch(
      componentFields(client, "Transform", { z: -10, qy: 1, qw: 0 }).map(
        (field) => ({
          kind: "setField",
          entity: { kind: "handle", id: camera },
          component: client.components.Transform!.id,
          field,
        }),
      ),
    ),
  );
  const turned = await enter("tab");
  expect(
    turned === "back:2",
    `Camera motion did not move keyboard entry: ${turned}`,
  );
  await press("escape");
  await host.detachWorld();
  return { entry, traversal, turned };
}

type GuiEditNode = Parameters<GuiTestClient["editGui"]>[0] & {
  action: "insert";
};

/**
 * Nested ScrollViews in their own World, observed through committed control
 * values: a tap at a fixed logical point toggles whichever checkbox the
 * committed scroll offsets moved under it.
 *
 * The 4x3 panel holds an outer ScrollView (4x3 viewport over 5 units of
 * content) whose content starts with an inner ScrollView (4x2 viewport over
 * 3 units), then a narrow 1x2 spacer. The inner checkbox rides inner content
 * at y 2..3 and the outer checkbox rides outer content at y 4..5, so the
 * inner view can scroll by 1 and the outer view by 2.
 */
async function exerciseGuiScrolling(
  host: WorldPersistenceHostClient<GuiTestClient>,
) {
  const client = await host.createWorld({ symbolicId: "gui-scrolling" });
  const ref = { kind: "alias", alias: 1 } as const;
  const entity = aliasId(
    await client.batch([
      createEntity(1, "gui-scrolling-panel"),
      insertComponent(client, "Transform", ref),
      insertComponent(client, "Surface", ref, { width: 4, height: 3 }),
      insertComponent(client, "GuiRoot", ref),
    ]),
    1,
  );
  const { rootIncarnation } = await client.inspectGui({ entity });
  const column = { kind: "container", containerKind: "column" } as const;
  const scrollView = {
    kind: "container",
    containerKind: "scrollView",
  } as const;
  const sizedBox = { kind: "container", containerKind: "sizedBox" } as const;
  const nodes: Omit<GuiEditNode, "action" | "entity" | "rootIncarnation">[] = [
    { id: 1, index: 0, data: column, style: { width: 4, height: 3 } },
    {
      id: 2,
      parent: 1,
      index: 0,
      data: scrollView,
      style: { width: 4, height: 3 },
    },
    { id: 3, parent: 2, index: 0, data: column },
    {
      id: 4,
      parent: 3,
      index: 0,
      data: scrollView,
      style: { width: 4, height: 2 },
    },
    { id: 5, parent: 4, index: 0, data: column },
    {
      id: 6,
      parent: 5,
      index: 0,
      data: sizedBox,
      style: { width: 4, height: 2 },
    },
    {
      id: 7,
      parent: 5,
      index: 1,
      data: { kind: "checkbox" },
      values: { checked: false },
      style: { width: 4, height: 1 },
    },
    {
      id: 8,
      parent: 3,
      index: 1,
      data: sizedBox,
      style: { width: 1, height: 2 },
    },
    {
      id: 9,
      parent: 3,
      index: 2,
      data: { kind: "checkbox" },
      values: { checked: false },
      style: { width: 4, height: 1 },
    },
  ];
  for (const node of nodes) {
    await client.editGui({
      action: "insert",
      entity,
      rootIncarnation,
      ...node,
    } as GuiEditNode);
  }
  const checked = async (id: number) => {
    const node = await inspected(client, entity, id);
    return node.controlValue.kind === "bool" && node.controlValue.value;
  };
  const tap = async (pointer: number, position: [number, number]) => {
    await client.submitGuiInput({
      kind: "pointerDown",
      pointer,
      position,
      button: "primary",
    });
    await client.submitGuiInput({
      kind: "pointerUp",
      pointer,
      position,
      button: "primary",
    });
  };

  // Two wheel samples over the inner view, pipelined so they may route in
  // one Host tick: the inner view takes its 1 unit and the rest passes
  // outward in both orders, exactly as if they routed in separate ticks.
  // The outer view then holds 2, lifting its checkbox to y 2..3.
  const wheel = (delta: [number, number]) =>
    client.submitGuiInput({ kind: "scroll", position: [2, 1], delta });
  const replies = await Promise.all([wheel([0, 1.5]), wheel([0, 1.5])]);
  expect(
    replies.every((reply) => reply.unhandled === undefined),
    `Nested wheel scrolling was unhandled: ${JSON.stringify(replies, (_, value) => (typeof value === "bigint" ? `${value}` : value))}`,
  );
  await tap(1, [2, 2.5]);
  expect(
    (await checked(9)) && !(await checked(7)),
    "Same-tick wheel chaining lost movement before the outer ScrollView",
  );

  // A touch drag over plain outer content scrolls it by the dragged
  // distance: two units down return the outer view to 0, bringing the
  // inner view (still scrolled by 1) back with its checkbox at y 1..2.
  const drag = async (
    pointer: number,
    from: [number, number],
    to: [number, number],
  ) => {
    const replies = [
      await client.submitGuiInput({
        kind: "pointerDown",
        pointer,
        position: from,
        button: "primary",
      }),
      await client.submitGuiInput({
        kind: "pointerMove",
        pointer,
        position: [from[0], (from[1] + to[1]) / 2],
      }),
      await client.submitGuiInput({
        kind: "pointerMove",
        pointer,
        position: to,
      }),
      await client.submitGuiInput({
        kind: "pointerUp",
        pointer,
        position: to,
        button: "primary",
      }),
    ];
    expect(
      replies.every((reply) => reply.unhandled === undefined),
      `A ScrollView drag was reported unhandled: ${JSON.stringify(replies, (_, value) => (typeof value === "bigint" ? `${value}` : value))}`,
    );
  };
  await drag(2, [2, 0.5], [2, 2.5]);
  await tap(3, [2, 1.5]);
  expect(
    (await checked(7)) && (await checked(9)),
    "A touch drag over ScrollView content did not scroll it",
  );

  // A drag starting on the inner checkbox wins over its tap: the inner view
  // is already at its end, so the unit of upward travel passes to the outer
  // view and the checkbox commits nothing. The outer shift then lifts the
  // checkbox to y 0..1.
  await drag(4, [2, 1.5], [2, 0.5]);
  expect(
    await checked(7),
    "A drag starting on a checkbox committed its toggle",
  );
  await tap(5, [2, 0.5]);
  expect(
    !(await checked(7)),
    "Drag travel beyond the inner ScrollView did not pass outward",
  );

  // Clips follow the outer scroll: with the outer view at 1 the inner
  // viewport spans y -1..1. Wheeling the inner view back to 0 leaves its
  // checkbox at y 1..2, below the moved viewport, so a tap there reaches
  // only plain outer content and toggles nothing.
  await client.submitGuiInput({
    kind: "scroll",
    position: [2, 0.5],
    delta: [0, -1],
  });
  await tap(6, [2, 1.5]);
  expect(
    !(await checked(7)),
    "A checkbox below the moved inner viewport was still hittable",
  );

  // Scrolling toward the start passes the inner view (already at 0) and
  // returns the outer view to 0; once every view sits at its edge the same
  // wheel is reported unhandled so scene controls can take it.
  const toStart = await wheel([0, -2]);
  expect(
    toStart.unhandled === undefined,
    "A partially consumed wheel was reported unhandled",
  );
  const atEdge = await wheel([0, -2]);
  expect(
    atEdge.unhandled?.kind === "scrollUnconsumed",
    `A wheel at every ScrollView edge was handled: ${JSON.stringify(atEdge.unhandled)}`,
  );

  // Scroll bars: the outer view's vertical bar spans x 3.85..4 with a
  // 1.8-unit thumb travelling 1.2 units over its capacity of 2. Semantic
  // snapshots report both ScrollViews' positions.
  const scrollOf = async (id: number) => {
    const tree = await client.semanticSnapshot({ entity });
    const node = tree.nodes.find((candidate) => candidate.id === id);
    expect(
      node?.role === "scrollView" && node.scroll !== undefined,
      `ScrollView ${id} has no semantic scroll position`,
    );
    return node.scroll;
  };
  let outer = await scrollOf(2);
  const inner = await scrollOf(4);
  expect(
    outer.offset[1] === 0 &&
      outer.maxOffset[1] === 2 &&
      inner.maxOffset[1] === 1,
    `Unexpected scroll positions: ${JSON.stringify({ outer, inner })}`,
  );
  const press = async (
    pointer: number,
    points: readonly [number, number][],
  ): Promise<void> => {
    const [first, ...rest] = points;
    const replies = [
      await client.submitGuiInput({
        kind: "pointerDown",
        pointer,
        position: first!,
        button: "primary",
      }),
    ];
    for (const position of rest)
      replies.push(
        await client.submitGuiInput({ kind: "pointerMove", pointer, position }),
      );
    replies.push(
      await client.submitGuiInput({
        kind: "pointerUp",
        pointer,
        position: points.at(-1)!,
        button: "primary",
      }),
    );
    expect(
      replies.every((reply) => reply.unhandled === undefined),
      "A scroll bar press was reported unhandled",
    );
  };
  // A track press below the thumb pages by the 3-unit viewport, clamped
  // to the end.
  await press(7, [[3.92, 2.5]]);
  outer = await scrollOf(2);
  expect(outer.offset[1] === 2, `Track paging missed: ${outer.offset}`);
  // Dragging the thumb (now at y 1.2..3) up by its whole travel returns
  // the view to the start, with the pointer leaving the bar on the way.
  await press(8, [
    [3.92, 2],
    [2, 1.4],
    [2, 0.8],
  ]);
  outer = await scrollOf(2);
  expect(
    Math.abs(outer.offset[1]) < 1e-4,
    `Thumb drag missed: ${outer.offset}`,
  );
  // Neither bar press toggled the checkboxes underneath.
  expect(
    !(await checked(7)) && (await checked(9)),
    "A scroll bar press reached content under the bar",
  );
  await host.detachWorld();
  return {
    outerMaxOffset: outer.maxOffset[1],
    innerMaxOffset: inner.maxOffset[1],
    edgeUnhandled: atEdge.unhandled?.kind,
    finalOuterOffset: outer.offset[1],
  };
}

/** Committed effects and cancellations of one node after batch `mark`. */
function nodeRecordsSince(
  log: ObservationLog,
  mark: number,
  entity: bigint,
  rootIncarnation: bigint,
  node: number,
) {
  const batches = log.batches.slice(mark);
  const matches = (target?: {
    entity: bigint;
    rootIncarnation: bigint;
    node: number;
  }) =>
    target !== undefined &&
    target.entity === entity &&
    target.rootIncarnation === rootIncarnation &&
    target.node === node;
  return {
    effects: batches.flatMap((batch) => batch.effects).filter(matches),
    cancellations: batches
      .flatMap((batch) => batch.cancellations ?? [])
      .filter((cancellation) => matches(cancellation.target))
      .map((cancellation) => cancellation.reason),
  };
}

/** Whether an operation rejected, for stale-handle assertions. */
async function rejected(operation: Promise<unknown>): Promise<boolean> {
  return await operation.then(
    () => false,
    () => true,
  );
}

/**
 * Remove GUI targets while they hold interaction state, in their own World:
 * a focused TextInput with an open composition, a slider captured mid-drag,
 * and then the whole GuiRoot mid-interaction. Each removal clears the
 * published text focus and capture, publishes no later effect for the
 * removed target, routes later input over the vacated area to its new
 * occupant and rejects the removed handles.
 *
 * The 4x3 panel stacks two 4x1.5 controls: the upper half at y 0..1.5 and
 * the lower half at y 1.5..3.
 */
async function exerciseGuiRemoval(
  host: WorldPersistenceHostClient<GuiTestClient>,
  fontBytes: ArrayBuffer,
) {
  const client = await host.createWorld({ symbolicId: "gui-removal" });
  const log = recordObservations(client);
  const font = await client.createAsset(17, fontBytes);
  const ref = { kind: "alias", alias: 1 } as const;
  const entity = aliasId(
    await client.batch([
      createEntity(1, "gui-removal-panel"),
      insertComponent(client, "Transform", ref),
      insertComponent(client, "Surface", ref, { width: 4, height: 3 }),
      insertComponent(client, "GuiRoot", ref),
    ]),
    1,
  );
  const upper: [number, number] = [2, 0.75];
  const lower: [number, number] = [2, 2.25];
  const half = { width: 4, height: 1.5 } as const;
  const populate = async (rootIncarnation: bigint) => {
    await client.editGuiBatch([
      {
        action: "insert",
        entity,
        rootIncarnation,
        id: 1,
        index: 0,
        data: { kind: "container", containerKind: "column" },
        style: { width: 4, height: 3 },
      },
      {
        action: "insert",
        entity,
        rootIncarnation,
        id: 2,
        parent: 1,
        index: 0,
        data: { kind: "textInput", text: "", placeholder: "" },
        style: { ...half, asset: font },
      },
      {
        action: "insert",
        entity,
        rootIncarnation,
        id: 3,
        parent: 1,
        index: 1,
        data: { kind: "slider" },
        values: { value: 0, min: 0, max: 1, step: 0 },
        style: half,
      },
    ]);
    // Programmatic focus is fenced against evaluated layout with a ready
    // font.
    await loadedFont(client);
    await client.waitForFrame();
  };
  let { rootIncarnation } = await client.inspectGui({ entity });
  await populate(rootIncarnation);
  const handle = (id: number, incarnation = rootIncarnation) =>
    client.createGuiNodeHandle(entity, incarnation, id);
  const composing = async () => {
    await client.submitGuiInput({ kind: "focus", handle: handle(2) });
    await client.submitGuiInput({ kind: "text", text: "ab" });
    await client.submitGuiInput({
      kind: "composition",
      text: "zz",
      caretStart: 2,
      caretEnd: 2,
    });
    return (await focusWhere(
      log,
      (state) =>
        state?.node === 2 &&
        state.rootIncarnation === rootIncarnation &&
        state.composition?.text === "zz",
      "The focused TextInput published no open composition",
    ))!;
  };
  const tapCommits = async (
    pointer: number,
    position: [number, number],
    node: number,
    incarnation: bigint,
  ) => {
    const mark = log.batches.length;
    await client.submitGuiInput({
      kind: "pointerDown",
      pointer,
      position,
      button: "primary",
    });
    await client.submitGuiInput({
      kind: "pointerUp",
      pointer,
      position,
      button: "primary",
    });
    return await eventually(() => {
      const found = nodeRecordsSince(
        log,
        mark,
        entity,
        incarnation,
        node,
      ).effects;
      return found.length > 0 ? found : undefined;
    }, `A tap over the vacated area did not reach node ${node}`);
  };

  // A focused TextInput with an open composition is removed: the text focus
  // clears, a later commit finds no focus, and the typed prefix never
  // publishes an effect for the removed node.
  const focus = await composing();
  const textMark = log.batches.length;
  await client.editGui({ action: "remove", handle: handle(2) });
  await focusWhere(
    log,
    (state) => state === null,
    "Removing the composing TextInput did not clear the text focus",
  );
  const lateCommit = await client.submitGuiInput({ kind: "commitComposition" });
  const lateText = await client.submitGuiInput({
    kind: "text",
    text: "late",
    fence: fenceOf(focus),
  });
  const textFocusGone =
    (await client.semanticSnapshot({ entity })).focused === undefined;
  expect(
    lateCommit.unhandled?.kind === "noFocus" && textFocusGone,
    `A composition commit after removal found a target: ${JSON.stringify(lateCommit.unhandled)}`,
  );
  // A native edit stamped with the removed focus conflicts.
  await eventually(
    () =>
      conflictsSince(log, textMark).includes("focusMismatch")
        ? true
        : undefined,
    `A stamped edit for the removed TextInput did not conflict: ${JSON.stringify(lateText.unhandled)}`,
  );
  const staleText = await rejected(
    client.editGui({
      action: "update",
      handle: handle(2),
      patch: { style: { opacity: 0.5 } },
    }),
  );
  expect(staleText, "The removed TextInput handle was accepted");

  // A slider captured mid-drag is removed: the rest of the drag commits
  // nothing, and a new checkbox in the vacated area takes later input. The
  // column reflowed the slider into the upper half.
  await client.waitForFrame();
  let sliderMark = log.batches.length;
  await client.submitGuiInput({
    kind: "pointerDown",
    pointer: 1,
    position: [1, upper[1]],
    button: "primary",
  });
  const move = await client.submitGuiInput({
    kind: "pointerMove",
    pointer: 1,
    position: [3, upper[1]],
  });
  // Wait for the move's own commit so no drag effect trails the removal.
  const dragged = await eventually(() => {
    const found = nodeRecordsSince(
      log,
      sliderMark,
      entity,
      rootIncarnation,
      3,
    ).effects;
    return found.some((effect) => effect.sourceTick === move.tick)
      ? found
      : undefined;
  }, "The captured slider drag committed no value");
  sliderMark = log.batches.length;
  await client.editGui({ action: "remove", handle: handle(3) });
  const afterSlider = [
    await client.submitGuiInput({
      kind: "pointerMove",
      pointer: 1,
      position: [2, upper[1]],
    }),
    await client.submitGuiInput({
      kind: "pointerUp",
      pointer: 1,
      position: [2, upper[1]],
      button: "primary",
    }),
  ];
  await client.editGui({
    action: "insert",
    entity,
    rootIncarnation,
    id: 4,
    parent: 1,
    index: 0,
    data: { kind: "checkbox" },
    values: { checked: false },
    style: { width: 4, height: 3 },
  });
  await client.waitForFrame();
  const vacated = await tapCommits(2, upper, 4, rootIncarnation);
  // Observations are ordered, so the new target's commit follows any stale
  // effect for either removed control.
  const textRecords = nodeRecordsSince(
    log,
    textMark,
    entity,
    rootIncarnation,
    2,
  );
  const sliderRecords = nodeRecordsSince(
    log,
    sliderMark,
    entity,
    rootIncarnation,
    3,
  );
  expect(
    textRecords.effects.length === 0 &&
      sliderRecords.effects.length === 0 &&
      sliderRecords.cancellations.includes("targetRemoved") &&
      afterSlider[1]!.unhandled?.kind === "noCapture" &&
      vacated.length === 1 &&
      vacated[0]!.kind === "controlCommitted" &&
      vacated[0]!.value.kind === "bool" &&
      vacated[0]!.value.value === true,
    `Removed controls left stale effects or misrouted later input: ${JSON.stringify({ textRecords, sliderRecords, afterSlider, vacated }, (_, value) => (typeof value === "bigint" ? `${value}` : value))}`,
  );
  const staleSlider = await rejected(
    client.editGui({
      action: "setControlValue",
      handle: handle(3),
      expectedRevision: 1,
      value: { kind: "scalar", value: 0.5 },
    }),
  );
  expect(staleSlider, "The removed slider handle was accepted");

  // The whole GuiRoot is removed while a TextInput composes and a press
  // holds the checkbox: focus and capture clear, the press completes
  // nothing, and every handle of the old incarnation is rejected. A new
  // GuiRoot on the same Surface has a fresh incarnation whose controls take
  // the same input.
  await client
    .editGuiBatch([
      {
        action: "update",
        handle: handle(4),
        patch: { style: { height: 1.5 } },
      },
      {
        action: "insert",
        entity,
        rootIncarnation,
        id: 2,
        parent: 1,
        index: 1,
        data: { kind: "textInput", text: "", placeholder: "" },
        style: { ...half, asset: font },
      },
    ])
    .then((outcome) => {
      // Node identities are never reused within an incarnation.
      expect(
        !outcome.ok && outcome.applied === 1,
        "A removed node id was reused",
      );
    });
  await client.editGui({
    action: "insert",
    entity,
    rootIncarnation,
    id: 5,
    parent: 1,
    index: 1,
    data: { kind: "textInput", text: "", placeholder: "" },
    style: { ...half, asset: font },
  });
  await client.waitForFrame();
  await client.submitGuiInput({ kind: "focus", handle: handle(5) });
  await client.submitGuiInput({
    kind: "composition",
    text: "qq",
    caretStart: 2,
    caretEnd: 2,
  });
  await focusWhere(
    log,
    (state) => state?.node === 5 && state.composition?.text === "qq",
    "The second TextInput published no open composition",
  );
  await client.submitGuiInput({
    kind: "pointerDown",
    pointer: 3,
    position: upper,
    button: "primary",
  });
  const oldIncarnation = rootIncarnation;
  const rootMark = log.batches.length;
  successfulBatch(
    await client.batch([
      {
        kind: "removeComponent",
        entity: { kind: "handle", id: entity },
        component: client.components.GuiRoot!.id,
      },
    ]),
  );
  await focusWhere(
    log,
    (state) => state === null,
    "Removing the GuiRoot did not clear the text focus",
  );
  const afterRoot = [
    await client.submitGuiInput({
      kind: "pointerUp",
      pointer: 3,
      position: upper,
      button: "primary",
    }),
    await client.submitGuiInput({ kind: "commitComposition" }),
  ];
  const staleRoot = [
    await rejected(
      client.editGui({
        action: "update",
        handle: handle(4, oldIncarnation),
        patch: { style: { opacity: 0.5 } },
      }),
    ),
    await rejected(
      client.editGui({
        action: "insert",
        entity,
        rootIncarnation: oldIncarnation,
        id: 6,
        index: 0,
        data: { kind: "container", containerKind: "column" },
      }),
    ),
  ];
  successfulBatch(
    await client.batch([
      insertComponent(client, "GuiRoot", { kind: "handle", id: entity }),
    ]),
  );
  ({ rootIncarnation } = await client.inspectGui({ entity }));
  expect(
    rootIncarnation !== oldIncarnation,
    "A replacement GuiRoot reused the removed incarnation",
  );
  staleRoot.push(
    await rejected(
      client.editGui({
        action: "update",
        handle: handle(4, oldIncarnation),
        patch: { style: { opacity: 0.5 } },
      }),
    ),
  );
  await populate(rootIncarnation);
  // The upper half now holds a fresh TextInput: a tap focuses it rather
  // than reaching the removed checkbox, and the lower slider commits.
  await client.submitGuiInput({
    kind: "pointerDown",
    pointer: 4,
    position: upper,
    button: "primary",
  });
  await client.submitGuiInput({
    kind: "pointerUp",
    pointer: 4,
    position: upper,
    button: "primary",
  });
  await focusWhere(
    log,
    (state) =>
      state?.node === 2 &&
      state.rootIncarnation === rootIncarnation &&
      state.composition === undefined,
    "A tap on the replacement root did not focus its TextInput",
  );
  const replacement = await tapCommits(5, [3, lower[1]], 3, rootIncarnation);
  const staleRootEffects = log.batches
    .slice(rootMark)
    .flatMap((batch) => batch.effects)
    .filter((effect) => effect.rootIncarnation === oldIncarnation);
  expect(
    staleRootEffects.length === 0 &&
      afterRoot[0]!.unhandled?.kind === "noCapture" &&
      afterRoot[1]!.unhandled?.kind === "noFocus" &&
      staleRoot.every(Boolean) &&
      replacement[0]!.kind === "controlCommitted",
    `Removing the GuiRoot left stale effects, accepted stale input or handles: ${JSON.stringify({ staleRootEffects, afterRoot, staleRoot }, (_, value) => (typeof value === "bigint" ? `${value}` : value))}`,
  );
  log.stop();
  await host.detachWorld();
  return {
    textRemoval: {
      lateCommit: lateCommit.unhandled?.kind,
      lateStampedEdit: "focusMismatch",
    },
    sliderRemoval: {
      draggedCommits: dragged.length,
      afterRemoval: afterSlider.map((reply) => reply.unhandled?.kind),
      cancellations: sliderRecords.cancellations,
    },
    rootRemoval: {
      afterRemoval: afterRoot.map((reply) => reply.unhandled?.kind),
      staleHandlesRejected: staleRoot.length,
      incarnations: [String(oldIncarnation), String(rootIncarnation)],
    },
  };
}
