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

  await host.detachWorld();
  await exerciseGuiScrolling(host);
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
}
