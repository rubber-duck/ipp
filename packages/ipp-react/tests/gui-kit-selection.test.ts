/**
 * Declarations of the GUI kit's selection controls (radio group, choice
 * reports, segmented control, tabs, tree view, dropdowns, multi-select and
 * autocomplete): which entities, components, links, themes and animation each
 * composition writes, through the real reconciler against the recording World
 * in `gui-kit-support.ts`. Rendered appearance is the skin lab's evidence
 * (`tests/gui/skin-lab/specimens/`); these tests pin the structure, the theme
 * references and the token arithmetic.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { createElement as h, useState } from "react";
import { Entity, createRoot } from "../src/index.js";
import {
  GUI_KIT_ICONS,
  GuiKit,
  RadioGroup,
  SegmentedControl,
  Tabs,
  TreeView,
  type RadioGroupProps,
  type SegmentedControlProps,
  type TabsProps,
  type TreeNode,
  type TreeViewHandle,
  type TreeViewProps,
  Autocomplete,
  Dropdown,
  MultiSelect,
  SearchableDropdown,
  selectionSummary,
  type AutocompleteProps,
  type DropdownProps,
  type MultiSelectProps,
  type SearchableDropdownProps,
  type SelectOption,
} from "../src/gui-kit.js";
import {
  TOKENS,
  PART,
  STATE,
  PRESSED,
  HOVERED,
  CHECK_MARK,
  contract,
  KitWorld,
  FONT,
  render,
  themeRows,
  settle,
  near,
  themeRow,
  checkedKey,
} from "./gui-kit-support.js";

const axisOptions: RadioGroupProps["options"] = [
  { value: "x", label: "X" },
  { value: "y", label: "Y" },
  { value: "z", label: "Z", disabled: true },
];

const axis = (props: Partial<RadioGroupProps> = {}) =>
  h(RadioGroup, {
    id: "axis",
    label: "AXIS",
    options: axisOptions,
    ...props,
  });

test("RadioGroup is a labelled group of radio marks whose arrows select", async () => {
  const { world, draw } = await render(axis({ defaultValue: "x" }), {
    fontSize: 32,
  });
  // A column of the caption and the options, at twice the design size.
  const root = world.fields("axis", "GuiLayout");
  assert.equal(root.get("kind"), 2);
  assert.equal(root.get("height"), 2 * (24 + 3 * 32));
  assert.deepEqual(world.children("axis"), ["axis/caption", "axis/options"]);
  assert.deepEqual(world.tone("axis/caption/text"), TOKENS.accent);
  // One group whose arrows move focus and select.
  const group = world.fields("axis/options", "GuiGroup");
  assert.equal(group.get("axis"), 1);
  assert.equal(group.get("selection"), 2);
  assert.deepEqual(world.children("axis/options"), [
    "axis/x",
    "axis/y",
    "axis/z",
  ]);
  // Each option is its mark, an icon-sized Button, beside its label.
  const option = world.fields("axis/x", "GuiLayout");
  assert.equal(option.get("height"), 64);
  near(option.get("width"), 48 + 16 + (0.54 * 32 + 0.32));
  assert.deepEqual(world.children("axis/x"), ["axis/x/strut", "axis/x/mark"]);
  for (const [value, selected, enabled] of [
    ["x", true, true],
    ["y", false, true],
    ["z", false, false],
  ] as const) {
    const mark = `axis/${value}/mark`;
    assert.equal(world.skin(mark), "radio");
    assert.equal(world.fields(mark, "GuiLayout").get("width"), 48);
    assert.equal(world.fields(mark, "GuiButton").get("label"), "");
    assert.equal(world.fields(mark, "GuiButton").get("selected"), selected);
    const behavior = world.fields(mark, "GuiBehavior");
    assert.equal(behavior.get("semantic_label"), value.toUpperCase());
    assert.equal(behavior.get("enabled"), enabled);
    // The label is a pointer-only Button inside the mark, so it is part of
    // the option's item. Its margins place it beside the mark, as wide as
    // its text, while its outer box stays the mark's width, which is all
    // the room the mark leaves a child: a long label neither overlaps the
    // mark nor loses presses past the mark's width.
    const label = `axis/${value}/label`;
    assert.deepEqual(world.children(mark), [label]);
    assert.equal(world.skin(label), "radioLabel");
    assert.equal(
      world.fields(label, "GuiButton").get("label"),
      value.toUpperCase(),
    );
    assert.equal(world.fields(label, "GuiBehavior").get("focusable"), false);
    const labelLayout = world.fields(label, "GuiLayout");
    const text = 0.54 * 32 + 0.32;
    near(labelLayout.get("width"), text);
    assert.equal(labelLayout.get("margin_left"), 48 + 16);
    near(labelLayout.get("margin_right"), -(16 + text));
  }

  // The mark: a circle round a dark interior, its focus ring on the same
  // circle; selected, a lit ring and a filled dot.
  assert.deepEqual(themeRow(world, "radio", PART.background), {
    part: PART.background,
    corner_cut: [0, 0, 0, 0],
    corner_radius: [12, 12],
  });
  assert.deepEqual(themeRow(world, "radio", PART.focusRing), {
    part: PART.focusRing,
    corner_cut: [0, 0, 0, 0],
    corner_radius: [12, 12],
  });
  assert.deepEqual(themeRow(world, "radio", PART.icon), {
    part: PART.icon,
    shape: 2,
    border_width: TOKENS.icon,
    color: [0, 0, 0, 0],
  });
  for (const [state, color] of [
    ["idle", TOKENS.accent],
    ["hovered", TOKENS.accent],
    ["pressed", TOKENS.surface],
    ["disabled", TOKENS.neutral],
  ] as const)
    assert.deepEqual(
      themeRow(world, "radio", checkedKey(PART.icon, state))?.color,
      color,
      state,
    );
  assert.deepEqual(
    themeRow(world, "radio", checkedKey(PART.background, "idle"))?.color,
    TOKENS.surface,
  );
  // The label paints nothing but its text, neutral while disabled.
  assert.deepEqual(themeRows(world, "radioLabel").rows, [
    [
      0,
      {
        part: PART.background,
        color: [0, 0, 0, 0],
        border_width: 0,
        glow_intensity: 0,
      },
    ],
    [1, { part: PART.label, color: TOKENS.text }],
    [2, { part: PART.label + STATE.disabled, color: TOKENS.neutral }],
  ]);

  // In a row, options follow each other an inset apart, without a caption.
  await draw(
    h(RadioGroup, {
      id: "axis",
      options: axisOptions,
      defaultValue: "x",
      horizontal: true,
    }),
  );
  assert.deepEqual(world.children("axis"), ["axis/options"]);
  assert.equal(world.fields("axis/options", "GuiLayout").get("kind"), 1);
  assert.equal(world.fields("axis/options", "GuiGroup").get("axis"), 0);
  assert.equal(world.fields("axis/y", "GuiLayout").get("margin_left"), 32);
});

test("a choice reports the runtime's selection and writes only the application's own", async () => {
  const changes: string[] = [];
  const onChange = (value: string) => changes.push(value);
  const { world, root, draw } = await render(axis({ value: "x", onChange }));
  await settle(root);

  // Arrows in the group select Y: the runtime writes both fields.
  world.value("axis/x/mark", "GuiButton", { selected: false });
  world.value("axis/y/mark", "GuiButton", { selected: true });
  await settle(root);
  assert.deepEqual(changes, ["y"]);
  // The application follows; nothing is written back, so a later arrow is
  // never undone by a stale write.
  await draw(axis({ value: "y", onChange }));
  await settle(root);
  assert.deepEqual(world.compared, []);
  assert.equal(world.fields("axis/x/mark", "GuiButton").get("selected"), true);
  assert.equal(world.fields("axis/y/mark", "GuiButton").get("selected"), false);

  // A value of the application's own is written to its item, and the
  // runtime's echo of it is not reported back.
  await draw(axis({ value: "x", onChange }));
  await settle(root);
  assert.deepEqual(world.compared, [["axis/x/mark", "selected", true]]);
  world.value("axis/x/mark", "GuiButton", { selected: true });
  world.value("axis/y/mark", "GuiButton", { selected: false });
  await settle(root);
  assert.deepEqual(changes, ["y"]);

  // A click on a label selects its option through the mark.
  world.effect("axis/y/label", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(world.compared.at(-1), ["axis/y/mark", "selected", true]);

  // Uncontrolled, the group keeps the reported selection itself.
  const own: string[] = [];
  const free = await render(
    axis({ defaultValue: "y", onChange: (value) => own.push(value) }),
  );
  await settle(free.root);
  free.world.value("axis/x/mark", "GuiButton", { selected: true });
  await settle(free.root);
  assert.deepEqual(own, ["x"]);
  assert.deepEqual(free.world.compared, []);
});

const space = (props: Partial<SegmentedControlProps> = {}) =>
  h(SegmentedControl, {
    id: "space",
    options: [
      { value: "world", label: "World" },
      { value: "local", label: "Local" },
      { value: "view", label: "View", disabled: true },
    ],
    defaultValue: "world",
    ...props,
  });

test("SegmentedControl shares one frame among segments cut only at its ends", async () => {
  const changes: string[] = [];
  const { world, root } = await render(
    space({ onChange: (value) => changes.push(value) }),
  );
  const frame = world.fields("space", "GuiLayout");
  assert.equal(frame.get("kind"), 1);
  assert.equal(frame.get("height"), 40);
  assert.equal(world.skin("space"), "frame");
  assert.equal(world.fields("space", "GuiGroup").get("axis"), 0);
  assert.equal(world.fields("space", "GuiGroup").get("selection"), 2);
  assert.deepEqual(world.children("space"), [
    "space/world",
    "space/local",
    "space/view",
  ]);
  for (const [value, theme, selected, enabled] of [
    ["world", "segmentFirst", true, true],
    ["local", "segment", false, true],
    ["view", "segmentLast", false, false],
  ] as const) {
    const segment = `space/${value}`;
    assert.equal(world.skin(segment), theme);
    assert.equal(world.fields(segment, "GuiLayout").get("flex"), 1);
    assert.equal(world.fields(segment, "GuiLayout").get("height"), 40);
    assert.equal(world.fields(segment, "GuiButton").get("selected"), selected);
    assert.equal(world.fields(segment, "GuiBehavior").get("enabled"), enabled);
  }
  assert.equal(world.fields("space/local", "GuiButton").get("label"), "Local");

  // Clear at rest; a segment after another starts with a quiet line; the
  // selected one fills solid; the frame's cut on the outer corners only.
  const base = (theme: string) => themeRow(world, theme, PART.background)!;
  assert.deepEqual(base("segmentFirst").corner_cut, [8, 0, 0, 0]);
  assert.equal(base("segmentFirst").fill_mode, 0);
  assert.deepEqual(base("segment").corner_cut, [0, 0, 0, 0]);
  assert.deepEqual(base("segmentLast").corner_cut, [0, 0, 8, 0]);
  assert.deepEqual(base("segmentOnly").corner_cut, [8, 0, 8, 0]);
  assert.deepEqual(
    {
      ...base("segment"),
      part: undefined,
    },
    {
      part: undefined,
      color: [0, 0, 0, 0],
      border_width: 0,
      corner_cut: [0, 0, 0, 0],
      fill_mode: 1,
      gradient_start: [1.25, 0],
      gradient_end: [1.26, 0],
      gradient_color0: TOKENS.line,
      gradient_color1: [0, 0, 0, 0],
    },
  );
  for (const state of ["idle", "hovered", "pressed", "disabled"] as const)
    assert.equal(
      themeRow(world, "segment", checkedKey(PART.background, state))?.fill_mode,
      0,
    );
  assert.deepEqual(
    themeRow(world, "segmentLast", PART.focusRing)?.corner_cut,
    [0, 0, 8, 0],
  );

  // Arrows select: the runtime's selection reaches onChange.
  await settle(root);
  world.value("space/local", "GuiButton", { selected: true });
  await settle(root);
  assert.deepEqual(changes, ["local"]);
});

const navigation = (props: Partial<TabsProps> = {}) =>
  h(Tabs, {
    id: "nav",
    tabs: [
      {
        value: "overview",
        label: "Overview",
        content: h(Entity, { id: "overview-body" }),
      },
      {
        value: "signals",
        label: "Signals",
        content: h(Entity, { id: "signals-body" }),
      },
      { value: "events", label: "Events", disabled: true },
    ],
    defaultValue: "signals",
    more: h(Entity, { id: "more" }),
    ...props,
  });

/** A ScrollView's runtime-written geometry. */
const scrollGeometry = (
  offset: number,
  viewport: number,
  capacity: number,
) => ({
  offset_x: offset,
  offset_y: 0,
  viewport_x: viewport,
  viewport_y: 40,
  content_x: viewport + capacity,
  content_y: 40,
  capacity_x: capacity,
  capacity_y: 0,
});

test("Tabs: a strip whose activation selects the tab whose content shows", async () => {
  const changes: string[] = [];
  const { world, root } = await render(
    navigation({ onChange: (value) => changes.push(value) }),
  );
  assert.deepEqual(world.children("nav"), [
    "nav/strip",
    "nav/line",
    "nav/content",
  ]);
  assert.equal(world.fields("nav/strip", "GuiLayout").get("height"), 40);
  // Without overflow the strip is its scroll view and the More slot.
  assert.deepEqual(world.children("nav/strip"), ["nav/view", "more"]);
  const view = world.fields("nav/view", "GuiScrollView");
  assert.equal(view.get("axis"), 0);
  assert.equal(view.get("bar_thickness"), 0);
  assert.equal(world.skin("nav/view"), "gridBody");
  assert.equal(world.fields("nav/view", "GuiLayout").get("flex"), 1);
  // One group whose activation selects, of tabs hugging their labels.
  const group = world.fields("nav/tabs", "GuiGroup");
  assert.equal(group.get("axis"), 0);
  assert.equal(group.get("selection"), 1);
  const widths = ["Overview", "Signals", "Events"].map(
    (label) => label.length * 0.54 * 16 + 0.16 + 32,
  );
  near(
    world.fields("nav/tabs", "GuiLayout").get("width"),
    widths.reduce((sum, width) => sum + width, 0),
  );
  for (const [value, selected, enabled] of [
    ["overview", false, true],
    ["signals", true, true],
    ["events", false, false],
  ] as const) {
    const tab = `nav/tab/${value}`;
    assert.equal(world.skin(tab), "tab");
    assert.equal(world.fields(tab, "GuiButton").get("selected"), selected);
    assert.equal(world.fields(tab, "GuiBehavior").get("enabled"), enabled);
  }
  near(world.fields("nav/tab/signals", "GuiLayout").get("width"), widths[1]!);
  // Only the selected tab's content is declared.
  assert.deepEqual(world.children("nav/content"), ["signals-body"]);
  assert.equal(world.entities.has("overview-body"), false);
  assert.equal(world.skin("nav/line"), "division");

  // Selected: the accent bar along the bottom edge and a lit label.
  assert.deepEqual(
    themeRow(world, "tab", checkedKey(PART.background, "idle")),
    {
      part: checkedKey(PART.background, "idle"),
      fill_mode: 1,
      gradient_start: [0, 36],
      gradient_end: [0, 36.01],
      gradient_color0: [0, 0, 0, 0],
      gradient_color1: TOKENS.accent,
    },
  );
  assert.deepEqual(
    themeRow(world, "tab", checkedKey(PART.label, "idle"))?.color,
    TOKENS.accent,
  );

  // Enter on Overview selects it: the content follows.
  await settle(root);
  world.value("nav/tab/overview", "GuiButton", { selected: true });
  await settle(root);
  assert.deepEqual(changes, ["overview"]);
  assert.deepEqual(world.children("nav/content"), ["overview-body"]);
  assert.equal(world.entities.has("signals-body"), false);

  // Overflowing, scroll buttons dock at the strip's ends, the one towards
  // the start disabled at the start; each scrolls half the strip.
  world.value("nav/view", "GuiScrollView", scrollGeometry(0, 200, 60));
  await settle(root);
  assert.deepEqual(world.children("nav/strip"), [
    "nav/previous",
    "nav/view",
    "nav/next",
    "more",
  ]);
  for (const [button, icon, enabled] of [
    ["nav/previous", GUI_KIT_ICONS.previous, false],
    ["nav/next", GUI_KIT_ICONS.next, true],
  ] as const) {
    assert.equal(world.skin(button), "dockedIcon");
    assert.equal(world.fields(button, "GuiButton").get("label"), icon);
    assert.equal(world.fields(button, "GuiBehavior").get("focusable"), false);
    assert.equal(world.fields(button, "GuiBehavior").get("enabled"), enabled);
    assert.equal(world.fields(button, "GuiLayout").get("width"), 32);
    assert.equal(world.fields(button, "GuiFont").get("font_size"), 24);
  }
  world.effect("nav/next", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(world.actions.at(-1), [
    "nav/view",
    { kind: "scrollBy", delta: [100, 0] },
  ]);
  world.value("nav/view", "GuiScrollView", scrollGeometry(60, 200, 60));
  await settle(root);
  assert.equal(world.fields("nav/next", "GuiBehavior").get("enabled"), false);
  assert.equal(
    world.fields("nav/previous", "GuiBehavior").get("enabled"),
    true,
  );
});

const sceneNodes: readonly TreeNode[] = [
  {
    key: "scene",
    label: "Scene",
    icon: "\u{f024b}",
    children: [
      {
        key: "camera",
        label: "Camera",
        children: [{ key: "lens", label: "Lens" }],
      },
      {
        key: "lighting",
        label: "Lighting",
        children: [
          { key: "key", label: "Key" },
          { key: "fill", label: "Fill" },
        ],
      },
      { key: "cube", label: "Cube", disabled: true },
    ],
  },
];

test("TreeView declares visible rows; unhandled Right and Left expand, collapse and move focus", async () => {
  const handle: { current: TreeViewHandle | null } = { current: null };
  const expansions: (readonly string[])[] = [];
  const selections: string[] = [];
  const tree = (props: Partial<TreeViewProps> = {}) =>
    h(TreeView, {
      id: "tree",
      nodes: sceneNodes,
      defaultExpanded: ["scene", "lighting"],
      onExpandedChange: (keys) => expansions.push(keys),
      onChange: (key) => selections.push(key),
      ref: handle,
      ...props,
    });
  const { world, root } = await render(tree());
  await settle(root);
  const rows = () => world.children("tree/rows");
  assert.deepEqual(rows(), [
    "tree/row/scene",
    "tree/row/camera",
    "tree/row/lighting",
    "tree/row/key",
    "tree/row/fill",
    "tree/row/cube",
  ]);
  // The rows are one group whose activation selects, in a list frame.
  assert.ok(world.entity("tree").components.has("GuiScrollView"));
  assert.equal(world.entity("tree").components.has("GuiSkin"), false);
  assert.equal(world.fields("tree/rows", "GuiGroup").get("axis"), 1);
  assert.equal(world.fields("tree/rows", "GuiGroup").get("selection"), 1);
  assert.equal(
    world.fields("tree/rows", "GuiLayout").get("height"),
    6 * 36 + 16,
  );
  // Indented a chevron column and a gap per level; the grid's row look.
  for (const [key, depth] of [
    ["scene", 0],
    ["camera", 1],
    ["key", 2],
  ] as const) {
    const row = `tree/row/${key}`;
    assert.equal(world.skin(row), "gridRow");
    assert.equal(world.fields(row, "GuiLayout").get("height"), 36);
    assert.equal(
      world.fields(row, "GuiLayout").get("padding_left"),
      8 + depth * 32,
    );
    assert.equal(world.fields(row, "GuiButton").get("selected"), false);
  }
  // Branches carry a pointer-only chevron Button, leaves its empty column.
  assert.deepEqual(world.children("tree/row/scene"), [
    "tree/row/scene/strut",
    "tree/row/scene/chevron",
    "tree/row/scene/icon",
    "tree/row/scene/label",
  ]);
  assert.equal(
    world.fields("tree/row/scene/chevron", "GuiButton").get("label"),
    GUI_KIT_ICONS.expanded,
  );
  assert.equal(
    world.fields("tree/row/camera/chevron", "GuiButton").get("label"),
    GUI_KIT_ICONS.collapsed,
  );
  assert.equal(world.skin("tree/row/camera/chevron"), "treeChevron");
  assert.equal(
    world.fields("tree/row/camera/chevron", "GuiBehavior").get("focusable"),
    false,
  );
  assert.deepEqual(world.children("tree/row/key"), [
    "tree/row/key/strut",
    "tree/row/key/leaf",
    "tree/row/key/label",
  ]);
  assert.equal(world.fields("tree/row/key/leaf", "GuiLayout").get("width"), 24);
  assert.equal(
    world.fields("tree/row/scene/icon", "CanvasText").get("text"),
    "\u{f024b}",
  );
  assert.equal(
    world.fields("tree/row/cube", "GuiBehavior").get("enabled"),
    false,
  );
  assert.deepEqual(world.tone("tree/row/cube/label"), TOKENS.neutral);

  // Keys reach the tree only while one of its rows holds focus.
  assert.equal(handle.current!.key("right"), false);
  const focus = (key: string, focused = true) =>
    world.effect(`tree/row/${key}`, {
      kind: "focusChanged",
      focused,
      changed: true,
      part: 0,
    });
  focus("camera");
  await settle(root);
  // Right expands a collapsed branch, then moves to its first child.
  assert.equal(handle.current!.key("right"), true);
  await settle(root);
  assert.deepEqual(expansions.at(-1), ["scene", "lighting", "camera"]);
  assert.deepEqual(rows().slice(1, 3), ["tree/row/camera", "tree/row/lens"]);
  assert.equal(
    world.fields("tree/row/lens", "GuiLayout").get("padding_left"),
    8 + 2 * 32,
  );
  assert.equal(handle.current!.key("right"), true);
  await settle(root);
  assert.deepEqual(world.actions.at(-1), ["tree/row/lens", { kind: "focus" }]);
  // Left on a leaf moves to its parent; on an expanded branch it collapses.
  focus("camera", false);
  focus("lens");
  await settle(root);
  assert.equal(handle.current!.key("right"), false);
  assert.equal(handle.current!.key("left"), true);
  await settle(root);
  assert.deepEqual(world.actions.at(-1), [
    "tree/row/camera",
    { kind: "focus" },
  ]);
  focus("lens", false);
  focus("camera");
  await settle(root);
  assert.equal(handle.current!.key("left"), true);
  await settle(root);
  assert.equal(world.entities.has("tree/row/lens"), false);
  assert.deepEqual(expansions.at(-1), ["scene", "lighting"]);
  // Left on a collapsed top-level branch has nowhere to go.
  focus("camera", false);
  focus("scene");
  await settle(root);
  assert.equal(handle.current!.key("up"), false);

  // A chevron collapses its branch without selecting; focus inside it moves
  // to the collapsed node.
  focus("scene", false);
  focus("fill");
  await settle(root);
  const actions = world.actions.length;
  world.effect("tree/row/lighting/chevron", { kind: "pressed" });
  await settle(root);
  assert.equal(world.entities.has("tree/row/fill"), false);
  assert.deepEqual(world.actions.slice(actions), [
    ["tree/row/lighting", { kind: "focus" }],
  ]);
  assert.deepEqual(world.compared, []);

  // Selection is the runtime's, reported once; it stays with a hidden row.
  world.value("tree/row/camera", "GuiButton", { selected: true });
  await settle(root);
  assert.deepEqual(selections, ["camera"]);
});

test("Tabs: a More menu while the strip overflows selects a tab and scrolls it into view", async () => {
  const changes: string[] = [];
  const { world, root } = await render(
    navigation({
      overflowMenu: true,
      onChange: (value) => changes.push(value),
    }),
  );
  assert.equal(world.entities.has("nav/more"), false);
  await settle(root);
  world.value("nav/view", "GuiScrollView", scrollGeometry(60, 200, 60));
  await settle(root);
  assert.deepEqual(world.children("nav/strip"), [
    "nav/previous",
    "nav/view",
    "nav/next",
    "nav/more",
    "more",
  ]);
  assert.equal(world.skin("nav/more"), "dockedIcon");
  assert.equal(world.fields("nav/more", "GuiBehavior").get("focusable"), false);
  assert.equal(
    world.fields("nav/more", "GuiButton").get("label"),
    GUI_KIT_ICONS.expanded,
  );
  // Its light menu lists every tab, below it and aligned with its end.
  const overlay = world.fields("nav/more/menu", "GuiOverlay");
  assert.deepEqual([overlay.get("align"), overlay.get("mode")], [2, 1]);
  assert.deepEqual(world.children("nav/more/items"), [
    "nav/more/items/overview",
    "nav/more/items/signals",
    "nav/more/items/events",
  ]);
  assert.equal(
    world.fields("nav/more/items/events", "GuiBehavior").get("enabled"),
    false,
  );
  world.effect("nav/more", { kind: "pressed" });
  await settle(root);
  assert.equal(
    world.fields("nav/more/menu", "GuiBehavior").get("visible"),
    true,
  );
  assert.equal(world.fields("nav/more", "GuiButton").get("selected"), true);
  // Overview lies left of the scrolled strip: it is selected and scrolled to.
  world.effect("nav/more/items/overview", { kind: "pressed" });
  await settle(root);
  assert.equal(
    world.fields("nav/more/menu", "GuiBehavior").get("visible"),
    false,
  );
  assert.deepEqual(world.compared.at(-1), [
    "nav/tab/overview",
    "selected",
    true,
  ]);
  assert.deepEqual(world.actions.at(-1), [
    "nav/view",
    { kind: "scrollTo", offset: [0, 0] },
  ]);
  world.value("nav/tab/overview", "GuiButton", { selected: true });
  await settle(root);
  assert.deepEqual(changes, ["overview"]);
});

test("selection looks: the trigger's open edge, its chevrons and the check mark of options that toggle", async () => {
  // A contract whose button look has a hover edge, which the trigger's open
  // look reuses.
  const edge = {
    part: HOVERED,
    border_width: TOKENS.litLineWidth,
    border_color: TOKENS.accent,
    glow_intensity: 0.02,
  };
  const base = contract();
  const world = new KitWorld();
  const root = createRoot(world);
  await root.render(
    h(
      GuiKit,
      {
        contract: {
          ...base,
          GUI_SKIN_LOOKS: {
            ...base.GUI_SKIN_LOOKS,
            button: { em: 16, parts: [edge] },
          },
        },
        font: FONT,
        fontSize: 16,
      },
      h(Entity, { id: "empty" }),
    ),
  );
  // Pressed, the surface stays under the hover edge; open, the checked
  // variant, the hover edge lies on the surface in every enabled state.
  assert.deepEqual(themeRow(world, "selectTrigger", PRESSED), {
    part: PRESSED,
    color: TOKENS.surface,
  });
  for (const state of ["idle", "hovered", "pressed"] as const)
    assert.deepEqual(
      themeRow(world, "selectTrigger", checkedKey(PART.background, state)),
      {
        ...edge,
        part: checkedKey(PART.background, state),
        color: TOKENS.surface,
      },
    );
  assert.deepEqual(
    themeRow(world, "selectTrigger", checkedKey(PART.background, "disabled")),
    {
      part: checkedKey(PART.background, "disabled"),
      color: TOKENS.surface,
      border_color: TOKENS.neutral,
    },
  );

  // The chevron: two strokes of the lit line meeting at its tip, each half
  // the line past it, pointing down, or up while open, or neutral.
  const past = TOKENS.litLineWidth / 2 / Math.SQRT2 / 16;
  const chevron = (name: string) =>
    themeRow(world, name, PART.background) as Record<string, number[]>;
  const down = chevron("selectChevron");
  assert.equal(down.shape, 1 as never);
  assert.equal(down.border_width, TOKENS.litLineWidth as never);
  assert.deepEqual(down.color, TOKENS.text);
  const close = (actual: number[] | undefined, expected: number[]) =>
    assert.ok(
      actual?.every(
        (value, index) => Math.abs(value - expected[index]!) < 1e-9,
      ),
      `${actual} != ${expected}`,
    );
  close(down.stroke_a, [2 / 16, 5 / 16, 0.5 + past, 11 / 16 + past]);
  close(down.stroke_b, [0.5 - past, 11 / 16 + past, 14 / 16, 5 / 16]);
  const up = chevron("selectChevronOpen");
  close(up.stroke_a, [2 / 16, 11 / 16, 0.5 + past, 5 / 16 - past]);
  close(up.stroke_b, [0.5 - past, 5 / 16 - past, 14 / 16, 11 / 16]);
  assert.deepEqual(chevron("selectChevronDisabled").color, TOKENS.neutral);

  // An option that toggles: a menu row without the selected bar, whose
  // icon is the check mark, clear unless selected, then lit, or neutral
  // while disabled.
  for (const state of ["idle", "hovered", "pressed", "disabled"] as const)
    assert.equal(
      themeRow(world, "optionCheckRow", checkedKey(PART.background, state)),
      undefined,
    );
  assert.deepEqual(themeRow(world, "optionCheckRow", HOVERED), {
    part: HOVERED,
    color: TOKENS.rowTint,
  });
  assert.deepEqual(themeRow(world, "optionCheckRow", PART.icon), {
    ...CHECK_MARK,
    color: [0, 0, 0, 0],
  });
  for (const state of ["idle", "hovered", "pressed"] as const)
    assert.deepEqual(
      themeRow(world, "optionCheckRow", checkedKey(PART.icon, state))?.color,
      TOKENS.accent,
    );
  assert.deepEqual(
    themeRow(world, "optionCheckRow", checkedKey(PART.icon, "disabled"))?.color,
    TOKENS.neutral,
  );
});

const SKINS: readonly SelectOption[] = [
  { key: "aurora", label: "Aurora" },
  { key: "ember", label: "Ember" },
  { key: "neon", label: "Neon" },
  { key: "static", label: "Static", disabled: true },
];

const dropdown = (props: Partial<DropdownProps> = {}) =>
  h(Dropdown, { id: "skin", label: "Skin", options: SKINS, ...props });

test("Dropdown: a field-like trigger and its light list of options that take no focus", async () => {
  const { world } = await render(dropdown({ defaultValue: "aurora" }));
  // The trigger: a Button in the trigger look, the control height, its
  // value at the content inset and the chevron at its end.
  assert.equal(world.skin("skin"), "selectTrigger");
  assert.equal(world.fields("skin", "GuiButton").get("label"), "");
  assert.equal(world.fields("skin", "GuiButton").get("selected"), false);
  assert.equal(
    world.fields("skin", "GuiBehavior").get("semantic_label"),
    "Skin: Aurora",
  );
  const trigger = world.fields("skin", "GuiLayout");
  assert.deepEqual(
    ["kind", "width", "height", "padding_left", "padding_right"].map((field) =>
      trigger.get(field),
    ),
    [1, 240, 40, 16, 16],
  );
  assert.equal(world.fields("skin", "GuiFont").get("font_size"), 16);
  assert.deepEqual(world.children("skin"), [
    "skin/strut",
    "skin/value",
    "skin/chevron",
    "skin/list",
  ]);
  assert.equal(world.fields("skin/value", "CanvasText").get("text"), "Aurora");
  assert.deepEqual(world.tone("skin/value"), TOKENS.text);
  assert.equal(world.skin("skin/chevron"), "selectChevron");
  const chevron = world.fields("skin/chevron", "GuiLayout");
  assert.deepEqual(
    ["width", "height", "margin_left"].map((field) => chevron.get(field)),
    [16, 16, 8],
  );

  // The list: an anchored light overlay stretched below the trigger a
  // quarter inset away, closed, on the floating surface.
  const overlay = world.fields("skin/list", "GuiOverlay");
  assert.deepEqual(
    ["side", "align", "mode"].map((field) => overlay.get(field)),
    [0, 3, 1],
  );
  assert.equal(world.fields("skin/list", "CanvasStyle").get("y"), 4);
  assert.equal(world.fields("skin/list", "GuiBehavior").get("visible"), false);
  assert.equal(world.skin("skin/list"), "floating");
  assert.deepEqual(world.children("skin/list"), ["skin/options"]);

  // The option list: a frameless scroll view as tall as its rows, a
  // vertical group without selection, rows in half-inset margins.
  assert.equal(world.skin("skin/options"), "gridBody");
  assert.equal(world.fields("skin/options", "GuiLayout").get("height"), 160);
  assert.equal(
    world.fields("skin/options", "GuiScrollView").get("bar_thickness"),
    0,
  );
  const group = world.fields("skin/options/rows", "GuiGroup");
  assert.equal(group.get("axis"), 1);
  assert.equal(group.get("selection"), undefined);
  const rows = world.fields("skin/options/rows", "GuiLayout");
  assert.deepEqual(
    ["height", "padding_top", "padding_left", "padding_right"].map((field) =>
      rows.get(field),
    ),
    [160, 8, 8, 8],
  );
  assert.deepEqual(
    world.children("skin/options/rows"),
    SKINS.map((option) => `skin/options/${option.key}`),
  );
  for (const [key, selected, enabled, tone] of [
    ["aurora", true, true, TOKENS.text],
    ["ember", false, true, TOKENS.text],
    ["static", false, false, TOKENS.neutral],
  ] as const) {
    const row = `skin/options/${key}`;
    assert.equal(world.skin(row), "menuRow");
    assert.equal(world.fields(row, "GuiButton").get("selected"), selected);
    const behavior = world.fields(row, "GuiBehavior");
    assert.equal(behavior.get("focusable"), false);
    assert.equal(behavior.get("enabled"), enabled);
    assert.equal(world.fields(row, "GuiLayout").get("height"), 36);
    assert.equal(world.fields(row, "GuiLayout").get("padding_left"), 8);
    assert.deepEqual(world.tone(`${row}/label`), tone);
  }

  // Without a selection the trigger shows its placeholder in the neutral
  // tone; disabled, its value and chevron are neutral.
  const { world: empty } = await render(dropdown({ placeholder: "Choose" }));
  assert.equal(empty.fields("skin/value", "CanvasText").get("text"), "Choose");
  assert.deepEqual(empty.tone("skin/value"), TOKENS.neutral);
  assert.equal(
    empty.children("skin/options/rows").some((row) => row.endsWith("empty")),
    false,
  );
  const { world: disabled } = await render(
    dropdown({ defaultValue: "neon", disabled: true }),
  );
  assert.equal(disabled.fields("skin", "GuiBehavior").get("enabled"), false);
  assert.deepEqual(disabled.tone("skin/value"), TOKENS.neutral);
  assert.equal(disabled.skin("skin/chevron"), "selectChevronDisabled");
});

test("Dropdown: the trigger toggles the list; a pick closes it and reports once", async () => {
  const changes: string[] = [];
  const opened: boolean[] = [];
  const { world, root } = await render(
    dropdown({
      defaultValue: "aurora",
      onChange: (key) => changes.push(key),
      onOpenChange: (open) => opened.push(open),
    }),
  );
  const visible = () => world.fields("skin/list", "GuiBehavior").get("visible");
  world.effect("skin", { kind: "pressed" });
  await settle(root);
  assert.equal(visible(), true);
  assert.equal(world.fields("skin", "GuiButton").get("selected"), true);
  assert.equal(world.skin("skin/chevron"), "selectChevronOpen");

  // Two picks before the first one's render: one change, and the list
  // closes; the trigger shows the new value.
  world.effect("skin/options/neon", { kind: "pressed" });
  world.effect("skin/options/ember", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(changes, ["neon"]);
  assert.equal(visible(), false);
  assert.equal(world.fields("skin", "GuiButton").get("selected"), false);
  assert.equal(world.fields("skin/value", "CanvasText").get("text"), "Neon");
  assert.equal(
    world.fields("skin/options/neon", "GuiButton").get("selected"),
    true,
  );
  assert.equal(
    world.fields("skin/options/aurora", "GuiButton").get("selected"),
    false,
  );

  // Picking the selected option closes the list without a change.
  world.effect("skin", { kind: "pressed" });
  await settle(root);
  world.effect("skin/options/neon", { kind: "pressed" });
  await settle(root);
  assert.equal(visible(), false);
  assert.deepEqual(changes, ["neon"]);

  // The runtime closing it, on Escape or an outside press, changes nothing;
  // the next press on the trigger opens it again.
  world.effect("skin", { kind: "pressed" });
  await settle(root);
  world.value("skin/list", "GuiBehavior", { visible: true });
  world.value("skin/list", "GuiBehavior", { visible: false });
  await settle(root);
  assert.equal(visible(), false);
  assert.deepEqual(changes, ["neon"]);
  world.effect("skin", { kind: "pressed" });
  await settle(root);
  assert.equal(visible(), true);
  assert.deepEqual(opened, [true, false, true, false, true, false, true]);
});

const NODES: readonly SelectOption[] = [
  { key: "alpha", label: "Alpha Station" },
  { key: "beta", label: "Beta Relay" },
  { key: "gamma", label: "Gamma Dock", disabled: true },
  { key: "galley", label: "Galley" },
];

const searchable = (props: Partial<SearchableDropdownProps> = {}) =>
  h(SearchableDropdown, { id: "node", options: NODES, ...props });

test("SearchableDropdown: a search field above the list filters it; Enter picks the first match", async () => {
  const changes: string[] = [];
  const queries: string[] = [];
  const { world, root } = await render(
    searchable({
      defaultValue: "alpha",
      onChange: (key) => changes.push(key),
      onQueryChange: (query) => queries.push(query),
    }),
  );
  // The surface holds the search field, a text input in a padded row with
  // a magnifier before its text, above the option list.
  assert.deepEqual(world.children("node/list"), [
    "node/search",
    "node/options",
  ]);
  const row = world.fields("node/search", "GuiLayout");
  assert.deepEqual(
    ["height", "padding_left", "padding_right", "padding_top"].map((field) =>
      row.get(field),
    ),
    [48, 8, 8, 8],
  );
  const field = "node/search/field";
  assert.equal(
    world.fields(field, "GuiTextInput").get("placeholder"),
    "Search",
  );
  const layout = world.fields(field, "GuiLayout");
  assert.deepEqual(
    ["height", "flex", "padding_left"].map((name) => layout.get(name)),
    [40, 1, 32],
  );
  assert.deepEqual(world.children(field), [`${field}/strut`, `${field}/icon`]);
  assert.equal(
    world.fields(`${field}/icon`, "CanvasText").get("text"),
    GUI_KIT_ICONS.search,
  );
  assert.equal(
    world.fields(`${field}/icon`, "GuiLayout").get("margin_left"),
    -16,
  );

  // Typing filters by label, ignoring case; the selection stays.
  world.effect("node", { kind: "pressed" });
  await settle(root);
  world.value(field, "GuiTextInput", { text: "GA" });
  await settle(root);
  assert.deepEqual(queries, ["GA"]);
  assert.deepEqual(world.children("node/options/rows"), [
    "node/options/gamma",
    "node/options/galley",
  ]);
  assert.equal(
    world.fields("node/value", "CanvasText").get("text"),
    "Alpha Station",
  );
  world.value(field, "GuiTextInput", { text: "zz" });
  await settle(root);
  assert.deepEqual(world.children("node/options/rows"), ["node/options/empty"]);
  assert.equal(
    world.fields("node/options/empty/label", "CanvasText").get("text"),
    "No results",
  );
  assert.deepEqual(world.tone("node/options/empty/label"), TOKENS.neutral);

  // Enter without an active option picks the first enabled match of the
  // submitted text, closes the list and clears the search.
  world.effect(field, { kind: "submitted", text: "ga" }, "GuiTextInput");
  await settle(root);
  assert.deepEqual(changes, ["galley"]);
  assert.equal(world.fields("node/list", "GuiBehavior").get("visible"), false);
  // The field lies in the closed list, which refuses actions: the search is
  // cleared with a write of its field, from the text it last reported.
  assert.deepEqual(world.compared.at(-1), [field, "text", ""]);
  world.value(field, "GuiTextInput", { text: "" });
  await settle(root);
  assert.deepEqual(queries, ["GA", "zz", ""]);
  assert.equal(world.children("node/options/rows").length, NODES.length);

  // A submission that matches nothing leaves the list open.
  world.effect("node", { kind: "pressed" });
  await settle(root);
  world.effect(field, { kind: "submitted", text: "zz" }, "GuiTextInput");
  await settle(root);
  assert.equal(world.fields("node/list", "GuiBehavior").get("visible"), true);
  assert.deepEqual(changes, ["galley"]);

  // Loading adds the spinner's row after the options.
  const { world: loading } = await render(searchable({ loading: "Loading…" }));
  assert.equal(
    loading.children("node/options/rows").at(-1),
    "node/options/loading",
  );
  assert.equal(
    loading
      .fields("node/options/loading/spinner/label", "CanvasText")
      .get("text"),
    "Loading…",
  );
  assert.equal(
    loading.fields("node/options", "GuiLayout").get("height"),
    (NODES.length + 1) * 36 + 16,
  );
});

const CHANNELS: readonly SelectOption[] = [
  { key: "render", label: "Render" },
  { key: "physics", label: "Physics" },
  { key: "network", label: "Network" },
];

const multi = (props: Partial<MultiSelectProps> = {}) =>
  h(MultiSelect, { id: "channels", options: CHANNELS, ...props });

test("MultiSelect: the trigger summarises the selection; options toggle with a check and the list stays open", async () => {
  // Labels while they fit, measured with the shared font, else the count.
  const advance = 0.54 * 16;
  assert.equal(
    selectionSummary(["Render", "Physics"], 15 * advance, 16),
    "Render, Physics",
  );
  assert.equal(
    selectionSummary(["Render", "Physics"], 15 * advance - 1, 16),
    "2 selected",
  );

  const changes: (readonly string[])[] = [];
  const { world, root, draw } = await render(
    multi({
      defaultValue: ["physics", "render"],
      onChange: (value) => changes.push(value),
    }),
  );
  // The default trigger leaves 240 - 2 * 16 - 16 - 8 units for its text.
  assert.equal(
    world.fields("channels/value", "CanvasText").get("text"),
    "Render, Physics",
  );
  for (const [key, selected] of [
    ["render", true],
    ["physics", true],
    ["network", false],
  ] as const) {
    const row = `channels/options/${key}`;
    assert.equal(world.skin(row), "optionCheckRow");
    assert.equal(world.fields(row, "GuiButton").get("selected"), selected);
    // The check takes the row's leading square.
    assert.equal(world.fields(row, "GuiLayout").get("padding_left"), 36);
  }

  // Toggles while open report the selection in option order, each built
  // on the one before even ahead of its render, and keep the list open.
  world.effect("channels", { kind: "pressed" });
  await settle(root);
  world.effect("channels/options/network", { kind: "pressed" });
  world.effect("channels/options/render", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(changes, [
    ["render", "physics", "network"],
    ["physics", "network"],
  ]);
  assert.equal(
    world.fields("channels/list", "GuiBehavior").get("visible"),
    true,
  );
  assert.equal(
    world.fields("channels/options/render", "GuiButton").get("selected"),
    false,
  );
  // Once the runtime closed the list, a late press toggles nothing.
  world.value("channels/list", "GuiBehavior", { visible: true });
  world.value("channels/list", "GuiBehavior", { visible: false });
  await settle(root);
  world.effect("channels/options/render", { kind: "pressed" });
  await settle(root);
  assert.equal(changes.length, 2);

  // Every channel does not fit: the count stands in; none shows the
  // placeholder.
  await draw(multi({ value: ["render", "physics", "network"] }));
  assert.equal(
    world.fields("channels/value", "CanvasText").get("text"),
    "3 selected",
  );
  await draw(multi({ value: [], placeholder: "Channels" }));
  assert.equal(
    world.fields("channels/value", "CanvasText").get("text"),
    "Channels",
  );
  assert.deepEqual(world.tone("channels/value"), TOKENS.neutral);
});

const PLACES: readonly SelectOption[] = [
  { key: "alpha", label: "Alpha Station" },
  { key: "alpine", label: "Alpine Relay" },
  { key: "beta", label: "Beta Relay" },
];

/** An application suggesting the places that start with the typed text. */
function Destination({
  log,
  ...props
}: Partial<AutocompleteProps> & { readonly log: unknown[] }) {
  const [text, setText] = useState("");
  const typed = text.toLowerCase();
  return h(Autocomplete, {
    id: "dest",
    suggestions: typed
      ? PLACES.filter((place) => place.label.toLowerCase().startsWith(typed))
      : [],
    onInputChange: (value) => {
      log.push(["input", value]);
      setText(value);
    },
    onSelect: (key) => log.push(["select", key]),
    onCommit: (value) => log.push(["commit", value]),
    ...props,
  });
}

test("Autocomplete: typing opens the application's suggestions; accepting writes the field once", async () => {
  const log: unknown[] = [];
  const { world, root } = await render(
    h(Destination, { log, placeholder: "Destination" }),
  );
  const visible = () => world.fields("dest/list", "GuiBehavior").get("visible");
  const typed = async (text: string) => {
    world.value("dest", "GuiTextInput", { text });
    await settle(root);
  };
  // The field: a text input of the control height holding the list.
  const input = world.fields("dest", "GuiTextInput");
  assert.equal(input.get("placeholder"), "Destination");
  assert.equal(input.get("text"), "");
  const layout = world.fields("dest", "GuiLayout");
  assert.deepEqual(
    ["kind", "width", "height"].map((field) => layout.get(field)),
    [0, 240, 40],
  );
  assert.deepEqual(world.children("dest"), ["dest/list"]);
  assert.equal(visible(), false);

  // Typing reports the text and shows the suggestions; none hide the list.
  await typed("al");
  assert.equal(visible(), true);
  assert.deepEqual(world.children("dest/suggestions/rows"), [
    "dest/suggestions/alpha",
    "dest/suggestions/alpine",
  ]);
  assert.equal(world.skin("dest/suggestions/alpha"), "menuRow");
  assert.equal(
    world.fields("dest/suggestions/alpha", "GuiButton").get("selected"),
    false,
  );
  world.value("dest/list", "GuiBehavior", { visible: true });
  await typed("alz");
  assert.equal(visible(), false);
  // The report of that hiding is the client's own: suggestions bring the
  // list back.
  world.value("dest/list", "GuiBehavior", { visible: false });
  await typed("alp");
  assert.equal(visible(), true);

  // Accepting a suggestion writes its label to the field, reports its key
  // and closes the list; the field's report of that label opens nothing.
  world.effect("dest/suggestions/alpine", { kind: "pressed" });
  world.effect("dest/suggestions/alpha", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(world.actions.at(-1), [
    "dest",
    { kind: "text", value: "Alpine Relay" },
  ]);
  assert.equal(visible(), false);
  await typed("Alpine Relay");
  assert.equal(visible(), false);

  // The runtime closing the list, on Escape or an outside press, keeps the
  // text; typing opens it again.
  await typed("b");
  assert.equal(visible(), true);
  world.value("dest/list", "GuiBehavior", { visible: true });
  world.value("dest/list", "GuiBehavior", { visible: false });
  await settle(root);
  assert.equal(visible(), false);
  await typed("be");
  assert.equal(visible(), true);

  // Enter without an active suggestion commits the typed text.
  world.effect("dest", { kind: "submitted", text: "be" }, "GuiTextInput");
  await settle(root);
  assert.equal(visible(), false);
  assert.deepEqual(log, [
    ["input", "al"],
    ["input", "alz"],
    ["input", "alp"],
    ["select", "alpine"],
    ["input", "Alpine Relay"],
    ["input", "b"],
    ["input", "be"],
    ["commit", "be"],
  ]);

  // Loading shows its row while the application has no suggestions yet.
  const { world: loading, root: loadingRoot } = await render(
    h(Destination, { log: [], loading: "Loading…" }),
  );
  loading.value("dest", "GuiTextInput", { text: "zz" });
  await settle(loadingRoot);
  assert.equal(loading.fields("dest/list", "GuiBehavior").get("visible"), true);
  assert.deepEqual(loading.children("dest/suggestions/rows"), [
    "dest/suggestions/loading",
  ]);
});
