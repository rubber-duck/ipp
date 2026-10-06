/**
 * Declarations of the GUI kit's containers (panel, separator, expander, window
 * controls and data grid): which entities, components, links, themes and
 * animation each composition writes, through the real reconciler against the
 * recording World in `gui-kit-support.ts`. Rendered appearance is the skin
 * lab's evidence (`tests/gui/skin-lab/specimens/`); these tests pin the structure,
 * the theme references and the token arithmetic.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { createElement as h } from "react";
import { Entity } from "../src/index.js";
import {
  DataGrid,
  Expander,
  GUI_KIT_ICONS,
  LabelledSeparator,
  Panel,
  PanelFooter,
  PanelHeader,
  SecondaryButton,
  Separator,
  TextLine,
  WindowControl,
  WindowControls,
  type DataGridColumn,
  type DataGridProps,
  type DataGridRow,
  type ExpanderProps,
} from "../src/gui-kit.js";
import {
  TOKENS,
  PART,
  STATE,
  CHECKED,
  PRESSED,
  render,
  themeRows,
  settle,
  near,
} from "./gui-kit-support.js";

test("Panel and Separator take the container and line themes", async () => {
  const { world } = await render(
    h(
      Panel,
      { id: "section", layout: { width: 200 } },
      h(Separator, { id: "rule", tone: "rule" }),
      h(Separator, { id: "divider", vertical: true }),
    ),
  );
  assert.equal(world.skin("section"), "container");
  assert.equal(world.fields("section", "GuiLayout").get("kind"), 2);
  assert.equal(world.fields("section", "GuiLayout").get("width"), 200);
  // Its parts lie inside its line.
  assert.equal(world.fields("section", "GuiLayout").get("padding_left"), 1.25);
  assert.equal(world.fields("section", "GuiLayout").get("padding_right"), 1.25);
  assert.equal(world.fields("section", "GuiFont").get("font_size"), 16);
  assert.deepEqual(world.children("section"), ["rule", "divider"]);
  assert.equal(world.skin("rule"), "rule");
  assert.equal(world.fields("rule", "GuiLayout").get("height"), 1.25);
  assert.equal(world.skin("divider"), "division");
  assert.equal(world.fields("divider", "GuiLayout").get("width"), 1.25);
  assert.deepEqual(themeRows(world, "container").rows[0]![1], {
    part: PART.background,
    color: TOKENS.page,
    border_width: TOKENS.lineWidth,
    border_color: TOKENS.accent,
    corner_accent: [8, 8, 8, 8],
    corner_accent_width: TOKENS.cornerAccentWidth,
  });
});

test("Expander declares its content after its header only while expanded", async () => {
  const content = h(Entity, { id: "gain" });
  const expander = (props: Omit<ExpanderProps, "children">) =>
    h(Expander, props, content);
  const { world, draw } = await render(
    expander({
      id: "advanced",
      label: "Advanced settings",
      summary: "3 options",
    }),
  );
  assert.equal(world.fields("advanced/header", "GuiButton").get("label"), "");
  assert.equal(
    world.fields("advanced/header", "GuiBehavior").get("semantic_label"),
    "Advanced settings",
  );
  assert.equal(
    world.fields("advanced/header", "GuiBehavior").get("enabled"),
    true,
  );
  assert.equal(world.skin("advanced/header"), "expanderHeader");
  assert.equal(world.fields("advanced/header", "GuiLayout").get("height"), 40);
  assert.deepEqual(world.children("advanced/header"), [
    "advanced/strut",
    "advanced/chevron",
    "advanced/label",
    "advanced/summary",
  ]);
  assert.equal(
    world.fields("advanced/chevron", "CanvasText").get("text"),
    GUI_KIT_ICONS.collapsed,
  );
  assert.deepEqual(world.tone("advanced/label"), TOKENS.accent);
  assert.deepEqual(world.tone("advanced/summary"), TOKENS.neutral);
  assert.equal(world.entities.has("gain"), false);

  // Controlled: the content follows the header in its container.
  await draw(
    expander({ id: "advanced", label: "Advanced settings", expanded: true }),
  );
  assert.deepEqual(world.children("advanced"), ["advanced/header", "gain"]);
  assert.equal(
    world.fields("advanced/chevron", "CanvasText").get("text"),
    GUI_KIT_ICONS.expanded,
  );
  assert.equal(world.entities.has("advanced/summary"), false);

  await draw(
    expander({
      id: "advanced",
      label: "Advanced settings",
      expanded: false,
      disabled: true,
    }),
  );
  assert.equal(world.entities.has("gain"), false);
  assert.equal(
    world.fields("advanced/header", "GuiBehavior").get("enabled"),
    false,
  );
  assert.deepEqual(world.tone("advanced/label"), TOKENS.neutral);

  // Uncontrolled: the initial expansion.
  const { world: open } = await render(
    expander({ id: "other", label: "Other", defaultExpanded: true }),
  );
  assert.deepEqual(open.children("other"), ["other/header", "gain"]);
});

test("Panel parts: a header strip with window controls, a labelled division and a footer row", async () => {
  const pressed: string[] = [];
  const press = (name: string) => () => pressed.push(name);
  const panel = (minimized: boolean) =>
    h(
      Panel,
      { id: "monitor", minimized, layout: { width: 400, height: 300 } },
      h(
        PanelHeader,
        { id: "monitor/header", title: "SIGNAL MONITOR" },
        h(
          WindowControls,
          minimized
            ? { id: "monitor/controls", onRestore: press("restore") }
            : {
                id: "monitor/controls",
                onMinimize: press("minimize"),
                onMaximize: press("maximize"),
                onClose: press("close"),
              },
        ),
      ),
      !minimized && [
        h(LabelledSeparator, {
          key: "events",
          id: "monitor/events",
          label: "EVENTS",
        }),
        h(
          PanelFooter,
          { key: "footer", id: "monitor/footer" },
          h(TextLine, {
            id: "monitor/count",
            text: "2 EVENTS",
            layout: { flex: 1 },
          }),
          h(SecondaryButton, {
            id: "monitor/clear",
            label: "CLEAR",
            onPress: press("clear"),
          }),
        ),
      ],
    );
  const { world, root, draw } = await render(panel(false), { fontSize: 32 });
  assert.equal(world.skin("monitor"), "container");
  assert.deepEqual(world.children("monitor"), [
    "monitor/header",
    "monitor/events",
    "monitor/footer",
  ]);

  // The header is a control-height strip: the title at the inset, the docked
  // buttons as far from its end as from its edges, then a division.
  const header = world.fields("monitor/header/row", "GuiLayout");
  assert.equal(header.get("height"), 80);
  assert.equal(header.get("padding_left"), 32);
  assert.equal(header.get("padding_right"), 16);
  assert.deepEqual(world.tone("monitor/header/title"), TOKENS.accent);
  assert.equal(
    world.fields("monitor/header/title", "GuiLayout").get("flex"),
    1,
  );
  assert.equal(world.skin("monitor/header/separator"), "division");
  assert.deepEqual(world.children("monitor/controls"), [
    "monitor/controls/minimize",
    "monitor/controls/maximize",
    "monitor/controls/close",
  ]);
  const controls = world.fields("monitor/controls", "GuiLayout");
  assert.equal(controls.get("width"), (3 * 32 + 2 * 8) * 2);
  assert.equal(controls.get("height"), 48);
  for (const kind of ["minimize", "maximize", "close"] as const) {
    const id = `monitor/controls/${kind}`;
    assert.equal(
      world.fields(id, "GuiButton").get("label"),
      GUI_KIT_ICONS[kind],
    );
    assert.equal(world.skin(id), "dockedIcon");
    assert.equal(world.fields(id, "GuiFont").get("font_size"), 48);
    assert.equal(world.fields(id, "GuiLayout").get("width"), 64);
    assert.equal(world.fields(id, "GuiLayout").get("height"), 48);
  }
  assert.equal(
    world.fields("monitor/controls/close", "GuiBehavior").get("semantic_label"),
    "Close",
  );
  assert.equal(
    world.fields("monitor/controls/maximize", "GuiLayout").get("margin_left"),
    16,
  );

  // A labelled division: a stub, the accent label and the rest of the line.
  assert.deepEqual(world.children("monitor/events"), [
    "monitor/events/strut",
    "monitor/events/stub",
    "monitor/events/label",
    "monitor/events/line",
  ]);
  assert.equal(world.fields("monitor/events", "GuiLayout").get("height"), 48);
  assert.equal(
    world.fields("monitor/events/stub", "GuiLayout").get("width"),
    32,
  );
  assert.equal(world.fields("monitor/events/line", "GuiLayout").get("flex"), 1);
  assert.equal(world.skin("monitor/events/line"), "division");
  assert.deepEqual(world.tone("monitor/events/label"), TOKENS.accent);

  // The footer: small buttons in half-inset margins at the content inset.
  const footer = world.fields("monitor/footer/row", "GuiLayout");
  assert.equal(footer.get("height"), (32 + 16) * 2);
  assert.equal(footer.get("padding_left"), 32);
  assert.equal(footer.get("padding_right"), 32);
  assert.equal(world.skin("monitor/clear"), "secondarySmall");
  assert.equal(world.fields("monitor/clear", "GuiLayout").get("height"), 64);
  near(
    world.fields("monitor/clear", "GuiLayout").get("width"),
    5 * 0.54 * 26 + 64,
  );

  // Presses reach the application's callbacks.
  world.effect("monitor/controls/close", { kind: "pressed" });
  world.effect("monitor/clear", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(pressed, ["close", "clear"]);

  // Minimised: the same frame unlit, as tall as its header, the title in the
  // text colour and no division.
  await draw(panel(true));
  assert.equal(world.skin("monitor"), "containerUnlit");
  assert.equal(world.fields("monitor", "GuiLayout").get("height"), 80);
  assert.deepEqual(world.tone("monitor/header/title"), TOKENS.text);
  assert.deepEqual(world.children("monitor"), ["monitor/header"]);
  assert.deepEqual(world.children("monitor/controls"), [
    "monitor/controls/restore",
  ]);
  assert.equal(world.fields("monitor/controls", "GuiLayout").get("width"), 64);
});

test("WindowControl stands free at the control height and takes the amber variant", async () => {
  const { world } = await render([
    h(WindowControl, { key: 1, id: "free", kind: "close", docked: false }),
    h(WindowControl, { key: 2, id: "docked", kind: "close", amber: true }),
    h(WindowControl, {
      key: 3,
      id: "off",
      kind: "close",
      docked: false,
      amber: true,
      disabled: true,
    }),
  ]);
  assert.equal(world.skin("free"), "secondaryIcon");
  assert.equal(world.fields("free", "GuiLayout").get("width"), 68);
  assert.equal(world.fields("free", "GuiLayout").get("height"), 40);
  assert.equal(world.fields("free", "GuiBehavior").get("enabled"), true);
  assert.equal(world.skin("docked"), "dockedIconAmber");
  assert.equal(world.skin("off"), "secondaryIconAmber");
  assert.equal(world.fields("off", "GuiBehavior").get("enabled"), false);
  // The docked amber look is the amber secondary look with square corners.
  assert.deepEqual(themeRows(world, "dockedIconAmber").rows, [
    [
      0,
      {
        part: PART.background,
        corner_cut: [0, 0, 0, 0],
        border_color: TOKENS.amber,
      },
    ],
  ]);
});

const COLUMNS: readonly DataGridColumn[] = [
  { key: "node", title: "NODE", width: 100 },
  { key: "signal", title: "SIGNAL", width: 96, align: "end" },
  { key: "status", title: "STATUS" },
];

const record = (node: string, signal: string, status: string): DataGridRow => ({
  key: node.toLowerCase(),
  cells: { node, signal, status },
});

const ROWS = [
  record("Alpha", "65%", "Online"),
  record("Bravo", "42%", "Standby"),
  record("Charlie", "88%", "Online"),
];

const grid = (props: Partial<DataGridProps>) =>
  h(DataGrid, {
    id: "grid",
    columns: COLUMNS,
    rows: ROWS,
    ...props,
  } as DataGridProps);

test("DataGrid lays out header, rows and lines; rows are Buttons the application selects", async () => {
  const pressed: string[] = [];
  const requested: unknown[] = [];
  const { world, root, draw } = await render(
    grid({
      sort: { column: "signal", direction: "descending" },
      selected: "bravo",
      focusedCell: { row: "bravo", column: "signal" },
      onRowPress: (key) => pressed.push(key),
      onRowContextMenu: (key, event) => requested.push([key, event.point]),
      footer: "1-3 of 3",
    }),
  );
  // The grid holds its rows: header, three rows and the footer line.
  assert.equal(world.fields("grid", "GuiLayout").get("height"), 36 * 4 + 24);
  assert.deepEqual(world.children("grid"), ["grid/table", "grid/footer"]);
  assert.deepEqual(world.children("grid/table"), [
    "grid/lines",
    "grid/header",
    "grid/body",
    "grid/bottom-line",
  ]);
  assert.equal(world.skin("grid/bottom-line"), "quiet");
  assert.equal(world.fields("grid/bottom-line", "GuiLayout").get("align_y"), 1);

  // Column lines once for the table, centred on each inner boundary; columns
  // take their width or share the rest.
  const lines = world.fields("grid/lines", "GuiLayout");
  assert.equal(lines.get("padding_left"), 4);
  assert.equal(lines.get("padding_right"), 0);
  assert.equal(world.fields("grid/lines/node", "GuiLayout").get("width"), 100);
  assert.equal(world.fields("grid/lines/status", "GuiLayout").get("flex"), 1);
  assert.deepEqual(world.children("grid/lines/status"), []);
  assert.equal(world.skin("grid/lines/node/line"), "quiet");
  const line = world.fields("grid/lines/node/line", "GuiLayout");
  assert.equal(line.get("width"), 1.25);
  assert.equal(line.get("margin_right"), -0.625);

  // The header: small accent titles, all at the start, and the sort marker.
  assert.deepEqual(world.tone("grid/header/signal/text"), TOKENS.accent);
  assert.equal(
    world.fields("grid/header/signal/text", "CanvasText").get("font_size"),
    13,
  );
  assert.equal(
    world.fields("grid/header/signal/text", "GuiLayout").get("align_x"),
    -1,
  );
  assert.equal(
    world.fields("grid/header/signal/sort", "CanvasText").get("text"),
    GUI_KIT_ICONS.sortDescending,
  );
  near(
    world.fields("grid/header/signal/sort", "GuiLayout").get("margin_left"),
    16 + 6 * 0.54 * 13 + 4,
  );
  assert.equal(world.entities.has("grid/header/node/sort"), false);

  // Rows: docked Buttons the row height tall, the selected one selected;
  // numbers end their cells.
  assert.deepEqual(world.children("grid/body"), [
    "grid/row/alpha",
    "grid/row/bravo",
    "grid/row/charlie",
  ]);
  assert.equal(world.skin("grid/row/bravo"), "gridRow");
  assert.equal(world.fields("grid/row/bravo", "GuiLayout").get("height"), 36);
  assert.equal(
    world.fields("grid/row/bravo", "GuiButton").get("selected"),
    true,
  );
  assert.equal(
    world.fields("grid/row/alpha", "GuiButton").get("selected"),
    false,
  );
  assert.equal(
    world.entity("grid/row/alpha").components.has("GuiBehavior"),
    false,
  );
  assert.deepEqual(world.children("grid/row/bravo"), [
    "grid/row/bravo/cells",
    "grid/row/bravo/line",
  ]);
  const cells = world.fields("grid/row/bravo/cells", "GuiLayout");
  assert.equal(cells.get("padding_left"), 4);
  assert.equal(cells.get("padding_bottom"), 1.25);
  assert.equal(
    world.fields("grid/row/bravo/signal", "GuiLayout").get("width"),
    96,
  );
  const end = world.fields("grid/row/bravo/signal/text", "GuiLayout");
  assert.equal(end.get("align_x"), 1);
  assert.equal(end.get("margin_right"), 16);
  const start = world.fields("grid/row/bravo/node/text", "GuiLayout");
  assert.equal(start.get("align_x"), -1);
  assert.equal(start.get("margin_left"), 16);
  assert.deepEqual(world.tone("grid/row/bravo/node/text"), TOKENS.text);
  assert.equal(
    world.fields("grid/row/bravo/node/text", "CanvasText").get("font_size"),
    16,
  );

  // The focused cell's edge sits on its four lines.
  assert.equal(world.skin("grid/row/bravo/signal/focus"), "cellFocus");
  const focus = world.fields("grid/row/bravo/signal/focus", "GuiLayout");
  assert.equal(focus.get("margin_left"), -0.75);
  assert.equal(focus.get("margin_right"), -0.75);
  assert.equal(focus.get("margin_top"), -1.375);
  assert.equal(world.entities.has("grid/row/alpha/signal/focus"), false);

  // The row look: nothing at rest, a legible press, and the selected row's
  // tint with the accent bar over the first line.
  const rows = new Map(
    themeRows(world, "gridRow").rows.map(([, row]) => [row.part, row]),
  );
  assert.deepEqual(rows.get(PART.background), {
    part: PART.background,
    color: [0, 0, 0, 0],
    border_width: 0,
    corner_cut: [0, 0, 0, 0],
  });
  assert.deepEqual(rows.get(PRESSED), { part: PRESSED, color: [0, 0, 0, 0] });
  for (const state of [STATE.idle, STATE.hovered, STATE.pressed])
    assert.deepEqual(rows.get(PART.background + state + CHECKED), {
      part: PART.background + state + CHECKED,
      fill_mode: 1,
      gradient_start: [4.625, 0],
      gradient_end: [4.635, 0],
      gradient_color0: TOKENS.accent,
      gradient_color1: TOKENS.rowTint,
    });

  // The footer's small text at the cell inset.
  assert.equal(
    world.fields("grid/footer/text", "CanvasText").get("text"),
    "1-3 of 3",
  );
  assert.equal(
    world.fields("grid/footer", "GuiLayout").get("padding_left"),
    20,
  );

  // A press names its row; the application owns the selection.
  world.effect("grid/row/charlie", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(pressed, ["charlie"]);
  // A context request names its row and keeps its point.
  world.effect("grid/row/alpha", { kind: "contextRequested", point: [40, 70] });
  await settle(root);
  assert.deepEqual(requested, [["alpha", [40, 70]]]);
  assert.deepEqual(pressed, ["charlie"]);
  assert.equal(
    world.fields("grid/row/bravo", "GuiButton").get("selected"),
    true,
  );

  // Rows that do not take focus, for a grid a group drives.
  await draw(grid({ focusableRows: false }));
  assert.equal(
    world.fields("grid/row/alpha", "GuiBehavior").get("focusable"),
    false,
  );
  assert.equal(
    world.fields("grid/row/bravo", "GuiButton").get("selected"),
    false,
  );
  assert.equal(world.entities.has("grid/footer"), false);
  assert.equal(world.fields("grid", "GuiLayout").get("height"), 36 * 4);

  // A grid that flexes takes its container's height instead.
  const { world: flexed } = await render(grid({ layout: { flex: 1 } }));
  assert.equal(flexed.fields("grid", "GuiLayout").has("height"), false);
  assert.equal(flexed.fields("grid", "GuiLayout").get("flex"), 1);
});

test("DataGrid scrolls, virtualises, edits a cell and shows the empty state", async () => {
  const many = Array.from({ length: 9 }, (_, index) =>
    record(`Node${index}`, `${index}%`, "Online"),
  );
  const scrollHandles: unknown[] = [];
  const { world, root, draw } = await render(
    grid({
      rows: many,
      scroll: true,
      scrollRef: (handle) => {
        if (handle) scrollHandles.push(handle);
      },
      layout: { height: 200 },
    }),
  );
  // The body's ScrollView hands its handle to the application.
  await settle(root);
  assert.equal(scrollHandles.length > 0, true);
  // A scrolling body takes the height it is given and keeps its bar in a
  // column of its own after the last column.
  assert.equal(world.fields("grid", "GuiLayout").get("height"), 200);
  const body = world.fields("grid/body", "GuiScrollView");
  assert.equal(body.get("bar_thickness"), 8);
  assert.equal(body.get("bar_inset"), 8);
  assert.equal(body.get("bar_end_inset"), 0);
  assert.equal(world.skin("grid/body"), "gridBody");
  assert.equal(
    world.fields("grid/body/rows", "GuiLayout").get("padding_right"),
    24,
  );
  assert.equal(world.children("grid/body/rows").length, 9);
  assert.equal(
    world.fields("grid/lines", "GuiLayout").get("padding_right"),
    24,
  );
  assert.equal(world.children("grid/lines/status").length, 2);
  assert.equal(
    world.fields("grid/header/cells", "GuiLayout").get("padding_right"),
    24,
  );
  assert.equal(
    world.fields("grid/row/node0/cells", "GuiLayout").get("padding_right"),
    0,
  );
  assert.equal(
    world.fields("grid/bottom-line", "GuiLayout").get("margin_right"),
    24,
  );
  const gridBody = new Map(
    themeRows(world, "gridBody").rows.map(([, row]) => [row.part, row]),
  );
  assert.deepEqual(gridBody.get(PART.background), {
    part: PART.background,
    border_width: 0,
    color: [0, 0, 0, 0],
  });

  // A virtual list realises the rows the runtime asks for, the row height each.
  await draw(
    h(DataGrid, {
      id: "grid",
      columns: COLUMNS,
      rowCount: 500,
      row: (index: number) => record(`Node${index}`, "1%", "Online"),
      layout: { height: 200 },
    }),
  );
  const list = world.fields("grid/body", "GuiVirtualList");
  assert.equal(list.get("item_count"), 500);
  assert.equal(list.get("item_extent"), 36);
  assert.equal(list.get("bar_inset"), 8);

  // The editing cell holds a text input in the square text input look that
  // takes focus when it appears; Enter reports its text.
  const edits: unknown[] = [];
  await draw(
    grid({
      editingCell: { row: "bravo", column: "signal" },
      onCellEdit: (cell, text) => edits.push(cell, text),
    }),
  );
  await settle(root);
  assert.equal(world.entities.has("grid/row/bravo/signal/text"), false);
  assert.equal(world.skin("grid/row/bravo/signal/editor"), "cellEditor");
  assert.equal(
    world.fields("grid/row/bravo/signal/editor", "GuiTextInput").get("text"),
    "42%",
  );
  assert.deepEqual(world.actions, [
    ["grid/row/bravo/signal/editor", { kind: "focus" }],
  ]);
  assert.deepEqual(themeRows(world, "cellEditor").rows, [
    [0, { part: PART.background, corner_cut: [0, 0, 0, 0] }],
  ]);
  world.effect(
    "grid/row/bravo/signal/editor",
    { kind: "submitted", text: "41%" },
    "GuiTextInput",
  );
  await settle(root);
  assert.deepEqual(edits, [{ row: "bravo", column: "signal" }, "41%"]);

  // Without rows the body shows the empty state, as tall as it needs.
  await draw(grid({ rows: [], scroll: true, emptyText: "No nodes" }));
  assert.deepEqual(world.children("grid/body"), ["grid/body/empty"]);
  assert.equal(
    world.fields("grid/body/empty/text", "CanvasText").get("text"),
    "No nodes",
  );
  assert.equal(world.fields("grid", "GuiLayout").get("height"), 36 + 40 + 32);
  assert.equal(world.fields("grid/lines", "GuiLayout").get("padding_right"), 0);
  const empty = world.fields("grid/body", "GuiLayout");
  assert.deepEqual(
    ["top", "right", "bottom", "left"].map((side) =>
      empty.get(`padding_${side}`),
    ),
    [16, 16, 16, 4 + 16],
  );

  // Rows that replace the empty state lose its inset: the body is the same
  // entity, and its row forms name their padding as well.
  for (const scroll of [false, true]) {
    await draw(grid({ rows: [], scroll }));
    await draw(grid({ scroll }));
    const rows = world.fields("grid/body", "GuiLayout");
    assert.deepEqual(
      ["top", "right", "bottom", "left"].map((side) =>
        rows.get(`padding_${side}`),
      ),
      [0, 0, 0, 0],
      scroll ? "scrolled rows" : "rows",
    );
    assert.equal(rows.get("margin_top"), 36);
  }
});
