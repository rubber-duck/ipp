/** GUI pointer/keyboard/text input through a generated client, independent of process launch and wire layout. */
import type { WorldPersistenceHostClient } from "@ipp/client";
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
  const handle = (id: number, lifetime: number) =>
    client.createGuiNodeHandle(entity, rootIncarnation, id, lifetime);

  await client.editGui({
    action: "insert",
    entity,
    rootIncarnation,
    id: 1,
    index: 0,
    content: { kind: "container", containerKind: "column" },
    style: { width: 4, height: 3 },
  });
  await client.editGui({
    action: "insert",
    entity,
    rootIncarnation,
    id: 2,
    parent: 1,
    index: 0,
    content: { kind: "checkbox", checked: false },
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
    handle: handle(2, checkbox.lifetime),
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
    handle: handle(2, checkbox.lifetime),
    patch: { style: { height: 1.5 } },
  });
  await client.editGui({
    action: "insert",
    entity,
    rootIncarnation,
    id: 3,
    parent: 1,
    index: 1,
    content: { kind: "textInput", text: "", placeholder: "" },
    style: { width: 4, height: 1.5, asset: font },
  });
  let field = await inspected(client, entity, 3);
  await client.submitGuiInput({
    kind: "focus",
    handle: handle(3, field.lifetime),
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
    handle: handle(2, checkbox.lifetime),
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
    handle: handle(3, field.lifetime),
  });
  await client.submitGuiInput({ kind: "cancelComposition" });

  // A delayed native range is fenced to its revision: an equal-length
  // external replace ("Hp世界" is 8 bytes, like "Hpqrstuv") moves the
  // revision, so the stale range conflicts instead of rebasing and the
  // next insert lands at the reset end.
  field = await inspected(client, entity, 3);
  await client.editGui({
    action: "setControlValue",
    handle: handle(3, field.lifetime),
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
    handle: handle(3, field.lifetime),
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

  // Scroll admits at the same logical point without reflowing layout.
  await client.submitGuiInput({
    kind: "scroll",
    position: at,
    delta: [0, -4],
  });
}
