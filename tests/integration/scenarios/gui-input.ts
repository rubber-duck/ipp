/** GUI pointer/keyboard/text input through a generated client, independent of process launch and wire layout. */
import type {
  GuiObservationBatch,
  GuiTextFence,
  GuiTextFocusState,
  WorldPersistenceHostClient,
} from "@ipp/client";
import { aliasId, createEntity, insertComponent } from "../camera-fixtures.js";
import type { GuiTestClient } from "./gui-lifecycle.js";

function expect(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
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

  // Keys without focus admit without effect; focusing the checkbox then
  // pressing Enter toggles it back on, and blur clears the focus.
  await client.submitGuiInput({ kind: "key", key: "tab", pressed: true });
  await client.submitGuiInput({
    kind: "focus",
    handle: handle(2),
  });
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
  // updates paint only (browser/GLES captures are env-blocked, so this
  // scenario asserts the committed seams headless instead).
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

  // Scroll admits at the same logical point without reflowing layout.
  await client.submitGuiInput({
    kind: "scroll",
    position: at,
    delta: [0, -4],
  });
  log.stop();
}
