/**
 * Declarations of the GUI kit's overlays (toasts, the floating surface, menu,
 * context menu, popover, tooltip and confirmation dialog): which entities,
 * components, links, themes and animation each composition writes, through the
 * real reconciler against the recording World in `gui-kit-support.ts`.
 * Rendered appearance is the skin lab's evidence
 * (`tests/gui/skin-lab/specimens/`); these tests pin the structure, the theme
 * references and the token arithmetic.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { Fragment, createElement as h } from "react";
import { Children, Entity } from "../src/index.js";
import {
  GUI_KIT_ICONS,
  TextLine,
  ToastStack,
  type ToastItem,
  type ToastStackProps,
  Floating,
  GUI_KIT_OVERLAY_BANDS,
  Menu,
  useOverlayOpen,
  type MenuItem,
  type OverlayOpenProps,
  ConfirmationDialog,
  ContextMenu,
  Popover,
  Tooltip,
  useContextMenu,
  type ConfirmationDialogProps,
} from "../src/gui-kit.js";
import { Button } from "../src/gui/controls.js";
import {
  TOKENS,
  PART,
  PRESSED,
  HOVERED,
  DESCRIPTORS,
  KitWorld,
  render,
  themeRows,
  settle,
  near,
  themeRow,
  checkedKey,
} from "./gui-kit-support.js";

const TOASTS: readonly ToastItem[] = [
  { key: "saved", severity: "success", text: "Scene saved" },
  {
    key: "upload",
    severity: "error",
    text: "Upload failed",
    action: { label: "Retry" },
  },
  { key: "note", text: "Third" },
];

const stack = (props: Partial<ToastStackProps>) =>
  h(ToastStack, { id: "toasts", toasts: TOASTS, ...props });

type Clip = {
  duration: number;
  tracks: {
    property: unknown;
    keys: { time: number; value: { value: number } }[];
  }[];
};

/** The keys of the one clip the World holds, as time and value. */
function clipKeys(world: KitWorld) {
  const clips = [...world.clips.values()] as Clip[];
  return clips.map((clip) =>
    clip.tracks[0]!.keys.map((key) => [key.time, key.value.value]),
  );
}

test("ToastStack is a top-level manual overlay of toasts in stable order", async () => {
  const { world, draw } = await render(stack({ limit: 2 }));
  // On the toast layer, above dialogs.
  const style = world.fields("toasts", "CanvasStyle");
  assert.equal(style.get("layer"), 0);
  assert.equal(
    world.fields("toasts", "GuiOverlay").get("band"),
    GUI_KIT_OVERLAY_BANDS.notification,
  );
  assert.equal(style.get("x"), -16);
  assert.equal(style.get("y"), -16);
  const overlay = world.fields("toasts", "GuiOverlay");
  assert.equal(overlay.get("side"), 0);
  assert.equal(overlay.get("align"), 2);
  assert.equal(world.fields("toasts", "GuiBehavior").get("visible"), true);
  assert.equal(world.fields("toasts", "GuiLayout").get("kind"), 2);
  assert.equal(world.fields("toasts", "GuiLayout").get("width"), 480);
  // The limit shows the first toasts; the rest wait.
  assert.deepEqual(world.children("toasts"), ["toasts/saved", "toasts/upload"]);

  // A toast is a non-focusable Button: a control row in half-inset margins.
  const saved = world.fields("toasts/saved", "GuiLayout");
  assert.equal(saved.get("kind"), 1);
  assert.equal(saved.get("height"), 56);
  assert.equal(world.fields("toasts/saved", "GuiButton").get("label"), "");
  const behavior = world.fields("toasts/saved", "GuiBehavior");
  assert.equal(behavior.get("focusable"), false);
  assert.equal(behavior.get("semantic_label"), "Scene saved");
  assert.equal(world.fields("toasts/saved", "CanvasStyle").get("opacity"), 1);
  assert.equal(world.skin("toasts/saved"), "toastAccent");
  assert.deepEqual(world.children("toasts/saved"), [
    "toasts/saved/strut",
    "toasts/saved/mark",
    "toasts/saved/text",
    "toasts/saved/close",
  ]);
  assert.equal(world.skin("toasts/saved/mark"), "checkLit");
  assert.equal(world.skin("toasts/saved/close"), "dockedIcon");
  assert.equal(
    world.fields("toasts/saved/close", "GuiBehavior").get("semantic_label"),
    "Dismiss",
  );

  // An error with an action: its colour and icon, the action, a divider.
  assert.equal(world.skin("toasts/upload"), "toastError");
  assert.equal(
    world.fields("toasts/upload", "GuiLayout").get("margin_top"),
    16,
  );
  assert.equal(
    world.fields("toasts/upload/mark", "CanvasText").get("text"),
    GUI_KIT_ICONS.error,
  );
  assert.deepEqual(world.tone("toasts/upload/mark"), TOKENS.error);
  assert.deepEqual(world.children("toasts/upload").slice(3), [
    "toasts/upload/action",
    "toasts/upload/divider",
    "toasts/upload/close",
  ]);
  assert.equal(world.skin("toasts/upload/action"), "secondarySmall");
  assert.equal(world.skin("toasts/upload/divider"), "division");
  // A toast floats over content: its tint lies on an opaque interior, the
  // surface with a hundredth of its role colour, blended in linear light.
  const interior = [
    ...TOKENS.surface
      .slice(0, 3)
      .map((value, channel) => value * 0.99 + TOKENS.error[channel]! * 0.01),
    1,
  ];
  assert.deepEqual(themeRows(world, "toastError").rows, [
    [
      PART.background,
      {
        part: PART.background,
        color: interior,
        border_color: TOKENS.error,
      },
    ],
    [
      1,
      {
        part: PRESSED,
        color: interior,
        border_color: TOKENS.error,
        glow_color: TOKENS.error,
      },
    ],
    [
      2,
      { part: HOVERED, border_color: TOKENS.error, glow_color: TOKENS.error },
    ],
  ]);

  // Only the success toast dismisses itself: it holds its opacity for six
  // seconds of the Host clock and fades out over a tenth of a second.
  assert.equal(world.controllers.size, 1);
  const [controller] = [...world.controllers.values()];
  const canvasStyle = DESCRIPTORS.CanvasStyle!;
  assert.deepEqual(controller!.drivers[0]!.property, {
    component: canvasStyle.id,
    offsets: [canvasStyle.fields.opacity!.offset],
  });
  assert.equal(controller!.looping, false);
  assert.deepEqual(world.controls, [{ action: "play" }]);
  assert.deepEqual(clipKeys(world), [
    [
      [0, 1],
      [6, 1],
      [6.1, 0],
    ],
  ]);

  // Other edges; an empty stack closes.
  await draw(stack({ side: "top", align: "start", toasts: [] }));
  assert.equal(world.fields("toasts", "GuiOverlay").get("side"), 1);
  assert.equal(world.fields("toasts", "GuiOverlay").get("align"), 0);
  assert.equal(world.fields("toasts", "CanvasStyle").get("x"), 16);
  assert.equal(world.fields("toasts", "CanvasStyle").get("y"), 16);
  assert.equal(world.fields("toasts", "GuiBehavior").get("visible"), false);
  assert.deepEqual(world.children("toasts"), []);
});

test("a toast dismisses on its close button or at the end of its time, paused while hovered or focused", async () => {
  const dismissed: string[] = [];
  const { world, root, draw } = await render(
    stack({ onDismiss: (key) => dismissed.push(key) }),
  );
  const [id] = [...world.controllers.keys()];
  assert.ok(id !== undefined, "no controller");

  // The close button reports its toast.
  world.effect("toasts/upload/close", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(dismissed, ["upload"]);

  // A hovering pointer or focus on a button pauses the time; play resumes
  // when neither remains.
  const hover = (symbol: string, hovered: boolean) =>
    world.effect(symbol, {
      kind: "interactionChanged",
      pointer: 1n,
      state: { hovered, pressed: false, captured: false },
      changed: true,
    });
  const controls = () =>
    world.controlled
      .filter(([controller]) => controller === id)
      .map(([, control]) => (control as { action: string }).action);
  hover("toasts/saved", true);
  await settle(root);
  assert.deepEqual(controls(), ["play", "pause"]);
  world.effect("toasts/saved/close", {
    kind: "focusChanged",
    focused: true,
    changed: true,
    part: 0,
  });
  hover("toasts/saved", false);
  await settle(root);
  assert.deepEqual(controls(), ["play", "pause"]);
  world.effect("toasts/saved/close", {
    kind: "focusChanged",
    focused: false,
    changed: true,
    part: 0,
  });
  await settle(root);
  assert.deepEqual(controls(), ["play", "pause", "play"]);

  // Completion on the Host clock ends the toast.
  world.complete(id);
  await settle(root);
  assert.deepEqual(dismissed, ["upload", "saved"]);

  // The time and the fade follow the stack's options; persistent toasts and
  // reduced motion change them.
  await draw(
    stack({
      toasts: [
        { key: "quick", text: "Quick" },
        { key: "kept", text: "Kept", persistent: true },
      ],
      duration: 1500,
    }),
    { reducedMotion: true },
  );
  const quick = [...world.controllers.values()];
  assert.equal(quick.length, 1);
  assert.deepEqual(clipKeys(world).at(-1), [
    [0, 1],
    [1.5, 0],
  ]);
});

test("the floating surface and menu rows: frames at rest, the active row tinted under the hover edge", async () => {
  const { world } = await render(h(Entity, { id: "empty" }));
  const frame = (cut: number) => [
    [
      PART.background,
      {
        part: PART.background,
        color: TOKENS.surface,
        border_width: TOKENS.lineWidth,
        border_color: TOKENS.neutral,
        corner_cut: [cut, 0, cut, 0],
      },
    ],
  ];
  assert.deepEqual(themeRows(world, "floating").rows, frame(TOKENS.cut));
  assert.deepEqual(themeRows(world, "floatingSmall").rows, frame(4));
  // A row paints nothing at rest; active and pressed, the row tint under
  // the docked look's hover edge; selected, a data grid row's bar and tint.
  assert.deepEqual(themeRow(world, "menuRow", PART.background), {
    part: PART.background,
    color: [0, 0, 0, 0],
    border_width: 0,
    corner_cut: [0, 0, 0, 0],
  });
  assert.deepEqual(themeRow(world, "menuRow", HOVERED), {
    part: HOVERED,
    color: TOKENS.rowTint,
  });
  assert.deepEqual(themeRow(world, "menuRow", PRESSED), {
    part: PRESSED,
    color: TOKENS.rowTint,
  });
  const bar = TOKENS.selectionGutter + TOKENS.lineWidth / 2;
  assert.deepEqual(
    themeRow(world, "menuRow", checkedKey(PART.background, "hovered")),
    {
      part: checkedKey(PART.background, "hovered"),
      fill_mode: 1,
      gradient_start: [bar, 0],
      gradient_end: [bar + 0.01, 0],
      gradient_color0: TOKENS.accent,
      gradient_color1: TOKENS.rowTint,
    },
  );
  assert.deepEqual(
    themeRow(world, "menuRow", checkedKey(PART.background, "disabled"))
      ?.gradient_color0,
    TOKENS.neutral,
  );
  // The amber row: the amber variant, square, its active row tinted amber.
  assert.deepEqual(themeRow(world, "menuRowAmber", PART.background), {
    part: PART.background,
    color: [0, 0, 0, 0],
    border_width: 0,
    corner_cut: [0, 0, 0, 0],
    border_color: TOKENS.amber,
  });
  assert.deepEqual(themeRow(world, "menuRowAmber", HOVERED), {
    part: HOVERED,
    color: [...TOKENS.amber.slice(0, 3), TOKENS.rowTint[3]],
  });
});

const COMMANDS: readonly MenuItem[] = [
  { key: "inspect", label: "Inspect", icon: "\u{f0214}" },
  { key: "duplicate", label: "Duplicate", icon: "\u{f018f}" },
  {
    key: "delete",
    label: "Delete",
    icon: "\u{f0a7a}",
    tone: "amber",
    separator: true,
  },
  { key: "unavailable", label: "Unavailable", disabled: true },
];

/**
 * A trigger whose light overlay holds a menu, as an application composes
 * them: the trigger toggles the overlay, the overlay follows the runtime's
 * writes, and activation closes it.
 */
function MenuHarness({
  log,
  ...open
}: OverlayOpenProps & { readonly log: unknown[] }) {
  const overlay = useOverlayOpen({
    ...open,
    onOpenChange: (value) => log.push(["open", value]),
  });
  return h(
    Entity,
    { id: "trigger" },
    h(Button, { label: "Menu", onPress: overlay.toggle }),
    h(
      Children,
      null,
      h(
        Floating,
        {
          id: "trigger/overlay",
          mode: "light",
          open: overlay.open,
          onVisibleChange: overlay.onVisibleChange,
        },
        h(Menu, {
          id: "trigger/menu",
          items: COMMANDS,
          overlay,
          onSelect: (key) => log.push(["select", key]),
        }),
      ),
    ),
  );
}

test("Menu: command rows that take no focus in a group, on a floating surface", async () => {
  const log: unknown[] = [];
  const { world } = await render(h(MenuHarness, { log, defaultOpen: true }));
  // The surface: an anchored light overlay one layer up, with the kit's
  // font, its column inside the frame's line.
  const overlay = world.fields("trigger/overlay", "GuiOverlay");
  assert.equal(overlay.get("side"), 0);
  assert.equal(overlay.get("align"), 0);
  assert.equal(overlay.get("mode"), 1);
  assert.equal(overlay.get("band"), GUI_KIT_OVERLAY_BANDS.popup);
  assert.equal(world.fields("trigger/overlay", "CanvasStyle").get("layer"), 0);
  assert.equal(
    world.fields("trigger/overlay", "GuiBehavior").get("visible"),
    true,
  );
  assert.equal(world.fields("trigger/overlay", "GuiFont").get("font_size"), 16);
  assert.equal(world.skin("trigger/overlay"), "floating");
  const surface = world.fields("trigger/overlay", "GuiLayout");
  assert.equal(surface.get("kind"), 2);
  assert.equal(surface.get("padding_left"), TOKENS.lineWidth);
  assert.equal(surface.get("padding_top"), TOKENS.lineWidth);

  // The list: a vertical group without selection, as wide as its longest
  // label needs, rows in half-inset margins and a separator before Delete.
  const group = world.fields("trigger/menu", "GuiGroup");
  assert.equal(group.get("axis"), 1);
  assert.equal(group.get("selection"), undefined);
  const list = world.fields("trigger/menu", "GuiLayout");
  near(list.get("width"), 8 + 8 + 8 + 24 + 8 + 11 * 0.54 * 16 + 0.16 + 16);
  near(list.get("height"), 16 + 4 * 36 + TOKENS.lineWidth + 8);
  assert.equal(list.get("padding_top"), 8);
  assert.deepEqual(world.children("trigger/menu"), [
    "trigger/menu/inspect",
    "trigger/menu/duplicate",
    "trigger/menu/delete/separator",
    "trigger/menu/delete",
    "trigger/menu/unavailable",
  ]);
  const separator = world.fields("trigger/menu/delete/separator", "GuiLayout");
  assert.equal(separator.get("height"), TOKENS.lineWidth);
  assert.equal(separator.get("margin_top"), 4);
  assert.equal(separator.get("margin_left"), 8);
  near(separator.get("width"), (list.get("width") as number) - 32);
  assert.equal(world.skin("trigger/menu/delete/separator"), "division");

  for (const [key, theme, tone, enabled] of [
    ["inspect", "menuRow", TOKENS.text, true],
    ["delete", "menuRowAmber", TOKENS.amber, true],
    ["unavailable", "menuRow", TOKENS.neutral, false],
  ] as const) {
    const row = `trigger/menu/${key}`;
    assert.equal(world.skin(row), theme);
    assert.equal(world.fields(row, "GuiLayout").get("height"), 36);
    assert.equal(world.fields(row, "GuiLayout").get("padding_left"), 8);
    assert.equal(world.fields(row, "GuiButton").get("label"), "");
    const behavior = world.fields(row, "GuiBehavior");
    assert.equal(behavior.get("focusable"), false);
    assert.equal(behavior.get("enabled"), enabled);
    assert.deepEqual(world.tone(`${row}/label`), tone);
    assert.deepEqual(world.tone(`${row}/icon`), tone);
    // Every row keeps the icon column, so labels align.
    assert.equal(world.fields(`${row}/icon`, "GuiLayout").get("width"), 24);
  }
  assert.deepEqual(world.children("trigger/menu/inspect"), [
    "trigger/menu/inspect/strut",
    "trigger/menu/inspect/icon",
    "trigger/menu/inspect/label",
  ]);
  assert.equal(
    world.fields("trigger/menu/inspect/icon", "CanvasText").get("text"),
    "\u{f0214}",
  );
  assert.equal(
    world.entity("trigger/menu/unavailable/icon").components.has("CanvasText"),
    false,
  );
  // A menu without icons has no icon column.
  const { world: plain } = await render(
    h(Menu, { id: "plain", items: [{ key: "a", label: "A" }] }),
  );
  assert.deepEqual(plain.children("plain/a"), [
    "plain/a/strut",
    "plain/a/label",
  ]);
  near(plain.fields("plain", "GuiLayout").get("width"), 160);
  assert.deepEqual(log, []);
});

test("Menu: activation closes its overlay and reports the command once", async () => {
  const log: unknown[] = [];
  const { world, root } = await render(
    h(MenuHarness, { log, defaultOpen: true }),
  );
  // Two presses before the first one's render: one command.
  world.effect("trigger/menu/duplicate", { kind: "pressed" });
  world.effect("trigger/menu/inspect", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(log, [
    ["open", false],
    ["select", "duplicate"],
  ]);
  assert.equal(
    world.fields("trigger/overlay", "GuiBehavior").get("visible"),
    false,
  );
  // The trigger opens it again, and the next activation reports again.
  world.effect("trigger", { kind: "pressed" });
  await settle(root);
  assert.equal(
    world.fields("trigger/overlay", "GuiBehavior").get("visible"),
    true,
  );
  world.effect("trigger/menu/inspect", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(log.slice(2), [
    ["open", true],
    ["open", false],
    ["select", "inspect"],
  ]);
});

test("useOverlayOpen follows the runtime closing its overlay, not an earlier report", async () => {
  const log: unknown[] = [];
  const { world, root } = await render(h(MenuHarness, { log }));
  const visible = () =>
    world.fields("trigger/overlay", "GuiBehavior").get("visible");
  assert.equal(visible(), false);
  await settle(root);
  world.effect("trigger", { kind: "pressed" });
  await settle(root);
  assert.equal(visible(), true);
  // The field's first report arrives after the opening and still shows the
  // declared closed state: it does not close the overlay again...
  world.value("trigger/overlay", "GuiBehavior", { visible: false });
  await settle(root);
  assert.equal(visible(), true);
  // ...but once the runtime reported it open, its closing is adopted, so the
  // next press on the trigger opens it rather than closing it.
  world.value("trigger/overlay", "GuiBehavior", { visible: true });
  world.value("trigger/overlay", "GuiBehavior", { visible: false });
  await settle(root);
  assert.equal(visible(), false);
  world.effect("trigger", { kind: "pressed" });
  await settle(root);
  assert.equal(visible(), true);
  assert.deepEqual(log, [
    ["open", true],
    ["open", false],
    ["open", true],
  ]);
});

/**
 * Targets that each declare their own context menu while it is open for
 * them, inside their declaration but outside its `Children`, so the menu is a
 * root of the canvas and unmounts with its target.
 */
function ContextHarness({
  log,
  targets = ["cube", "sphere"],
}: {
  readonly log: unknown[];
  readonly targets?: readonly string[];
}) {
  const menu = useContextMenu<string>();
  return h(
    Fragment,
    null,
    targets.map((target) =>
      h(
        Entity,
        { key: target, id: target },
        h(Button, { label: target, onContextMenu: menu.opener(target) }),
        menu.request?.target === target &&
          h(ContextMenu<string>, {
            id: `${target}/menu`,
            menu,
            items: COMMANDS,
            onSelect: (key, chosen) => log.push([key, chosen]),
          }),
      ),
    ),
  );
}

test("ContextMenu opens a light menu at the request's point for its target", async () => {
  const log: unknown[] = [];
  const { world, root, draw } = await render(h(ContextHarness, { log }));
  const request = (target: string, point: readonly [number, number]) =>
    world.effect(target, { kind: "contextRequested", point });
  assert.equal(world.entities.has("cube/menu"), false);
  request("cube", [40, 50]);
  await settle(root);
  // A root of the canvas at the point; the surface below it, flipping and
  // shifting to stay inside the canvas, holds the commands.
  assert.equal(world.entity("cube/menu").parent, null);
  const anchor = world.fields("cube/menu", "CanvasStyle");
  assert.equal(anchor.get("x"), 40);
  assert.equal(anchor.get("y"), 50);
  assert.equal(world.entity("cube/menu").components.has("GuiLayout"), false);
  assert.deepEqual(world.children("cube/menu"), ["cube/menu/surface"]);
  const overlay = world.fields("cube/menu/surface", "GuiOverlay");
  assert.deepEqual(
    [overlay.get("side"), overlay.get("align"), overlay.get("mode")],
    [0, 0, 1],
  );
  assert.equal(
    world.fields("cube/menu/surface", "GuiBehavior").get("visible"),
    true,
  );
  assert.deepEqual(world.children("cube/menu/surface"), ["cube/menu/items"]);
  assert.equal(world.skin("cube/menu/items/delete"), "menuRowAmber");

  // A command reports its request's target once, and the menu closes.
  world.effect("cube/menu/items/delete", { kind: "pressed" });
  world.effect("cube/menu/items/inspect", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(log, [["delete", "cube"]]);
  assert.equal(world.entities.has("cube/menu"), false);

  // The runtime closing it, on Escape or an outside press, runs nothing.
  request("cube", [10, 20]);
  await settle(root);
  assert.equal(world.fields("cube/menu", "CanvasStyle").get("x"), 10);
  world.value("cube/menu/surface", "GuiBehavior", { visible: true });
  world.value("cube/menu/surface", "GuiBehavior", { visible: false });
  await settle(root);
  assert.equal(world.entities.has("cube/menu"), false);

  // Another target's request moves the menu to it; the first target's menu
  // unmounting does not close the second's.
  request("cube", [10, 20]);
  await settle(root);
  request("sphere", [70, 20]);
  await settle(root);
  assert.equal(world.entities.has("cube/menu"), false);
  assert.equal(world.fields("sphere/menu", "CanvasStyle").get("x"), 70);
  assert.equal(
    world.fields("sphere/menu/surface", "GuiBehavior").get("visible"),
    true,
  );

  // The target going away takes its menu with it and closes the request,
  // so the target's return shows no menu and nothing ran.
  await draw(h(ContextHarness, { log, targets: ["cube"] }));
  assert.equal(world.entities.has("sphere/menu"), false);
  await draw(h(ContextHarness, { log, targets: ["cube", "sphere"] }));
  assert.equal(world.entities.has("sphere/menu"), false);
  assert.deepEqual(log, [["delete", "cube"]]);
});

test("Popover: a trigger with a caret opens a titled light surface of the application's content", async () => {
  const changes: boolean[] = [];
  const popover = (open?: boolean) =>
    h(
      Popover,
      {
        id: "options",
        label: "Options",
        title: "Options",
        ...(open === undefined ? {} : { open }),
        onOpenChange: (value) => changes.push(value),
      },
      h(TextLine, { id: "options-label", text: "Label" }),
    );
  const { world, root } = await render(popover());
  // The trigger: a secondary button at the small size with a caret.
  const label = `Options ${GUI_KIT_ICONS.sortDescending}`;
  assert.equal(world.fields("options", "GuiButton").get("label"), label);
  assert.equal(world.fields("options", "GuiButton").get("selected"), false);
  assert.equal(world.skin("options"), "secondarySmall");
  near(
    world.fields("options", "GuiLayout").get("width"),
    [...label].length * 0.54 * 13 + 32,
  );
  assert.equal(world.fields("options", "GuiLayout").get("height"), 32);
  // The surface below it, a quarter inset away, its header strip with the
  // title and a close button that takes no focus, then the content.
  assert.deepEqual(world.children("options"), ["options/popover"]);
  const overlay = world.fields("options/popover", "GuiOverlay");
  assert.deepEqual(
    [overlay.get("side"), overlay.get("align"), overlay.get("mode")],
    [0, 1, 1],
  );
  assert.equal(world.fields("options/popover", "CanvasStyle").get("y"), 4);
  assert.equal(world.fields("options/popover", "GuiLayout").get("width"), 240);
  assert.equal(world.skin("options/popover"), "floating");
  assert.deepEqual(world.children("options/popover"), [
    "options/header",
    "options/content",
  ]);
  assert.deepEqual(world.tone("options/header/title"), TOKENS.accent);
  const close = world.fields("options/close", "GuiBehavior");
  assert.equal(close.get("focusable"), false);
  assert.equal(world.skin("options/close"), "dockedIcon");
  assert.equal(
    world.fields("options/content", "GuiLayout").get("padding_left"),
    16,
  );
  assert.deepEqual(world.children("options/content"), ["options-label"]);
  const visible = () =>
    world.fields("options/popover", "GuiBehavior").get("visible");
  assert.equal(visible(), false);

  // The trigger toggles it and takes its open look; the close button and
  // the runtime close it.
  await settle(root);
  world.effect("options", { kind: "pressed" });
  await settle(root);
  assert.equal(visible(), true);
  assert.equal(world.fields("options", "GuiButton").get("selected"), true);
  world.effect("options/close", { kind: "pressed" });
  await settle(root);
  assert.equal(visible(), false);
  world.effect("options", { kind: "pressed" });
  await settle(root);
  world.value("options/popover", "GuiBehavior", { visible: true });
  world.value("options/popover", "GuiBehavior", { visible: false });
  await settle(root);
  assert.equal(visible(), false);
  assert.equal(world.fields("options", "GuiButton").get("selected"), false);
  assert.deepEqual(changes, [true, false, true, false]);
});

test("Tooltip: a closed hint of its control on the small surface", async () => {
  const { world } = await render(
    h(
      Entity,
      { id: "help" },
      h(Button, { label: "?" }),
      h(
        Children,
        null,
        h(Tooltip, {
          id: "help/tip",
          text: ["Gain controls signal", "strength."],
        }),
      ),
    ),
  );
  const overlay = world.fields("help/tip", "GuiOverlay");
  assert.deepEqual(
    [overlay.get("side"), overlay.get("align"), overlay.get("mode")],
    [1, 1, 3],
  );
  // Declared closed and never followed: the runtime opens and closes it.
  assert.equal(world.fields("help/tip", "GuiBehavior").get("visible"), false);
  assert.equal(world.skin("help/tip"), "floatingSmall");
  assert.equal(world.fields("help/tip", "CanvasStyle").get("y"), -4);
  const layout = world.fields("help/tip", "GuiLayout");
  near(layout.get("width"), 20 * 0.54 * 13 + 0.13 + 16);
  assert.equal(layout.get("padding_top"), 4);
  assert.equal(layout.get("padding_left"), 8);
  assert.deepEqual(world.children("help/tip"), [
    "help/tip/line/0",
    "help/tip/line/1",
  ]);
  assert.equal(world.fields("help/tip/line/1", "GuiLayout").get("height"), 24);
  assert.equal(
    world.fields("help/tip/line/1/text", "CanvasText").get("font_size"),
    13,
  );
  // Beside its control, the offset follows the side.
  const { world: side } = await render(
    h(
      Entity,
      { id: "help" },
      h(Button, { label: "?" }),
      h(
        Children,
        null,
        h(Tooltip, { id: "help/tip", text: "Gain", side: "left" }),
      ),
    ),
  );
  assert.equal(side.fields("help/tip", "GuiOverlay").get("side"), 3);
  assert.equal(side.fields("help/tip", "CanvasStyle").get("x"), -4);
});

test("ConfirmationDialog: a modal centred dialog whose answer is reported once per opening", async () => {
  const answers: string[] = [];
  const dialog = (props: Partial<ConfirmationDialogProps> = {}) =>
    h(ConfirmationDialog, {
      id: "confirm",
      open: true,
      title: "Delete node?",
      body: ["Delete Cube from the scene?", "This action cannot be undone."],
      action: "Delete",
      onConfirm: () => answers.push("confirm"),
      onCancel: () => answers.push("cancel"),
      ...props,
    });
  const { world, root, draw } = await render(dialog());
  const overlay = world.fields("confirm", "GuiOverlay");
  assert.deepEqual(
    [overlay.get("side"), overlay.get("align"), overlay.get("mode")],
    [4, 1, 2],
  );
  assert.equal(world.fields("confirm", "CanvasStyle").get("layer"), 0);
  assert.equal(overlay.get("band"), GUI_KIT_OVERLAY_BANDS.dialog);
  assert.equal(world.fields("confirm", "GuiLayout").get("width"), 368);
  assert.equal(world.fields("confirm", "GuiFont").get("font_size"), 16);
  assert.equal(world.skin("confirm"), "floating");
  assert.deepEqual(world.children("confirm"), [
    "confirm/header",
    "confirm/body",
  ]);
  // Close takes no focus, so Cancel, before the action, takes it first.
  assert.equal(
    world.fields("confirm/close", "GuiBehavior").get("focusable"),
    false,
  );
  assert.equal(
    world.fields("confirm/close", "GuiBehavior").get("semantic_label"),
    "Cancel",
  );
  assert.equal(
    world.fields("confirm/body", "GuiLayout").get("height"),
    16 + 2 * 24 + 16 + 40 + 16,
  );
  assert.deepEqual(world.children("confirm/body"), [
    "confirm/line/0",
    "confirm/line/1",
    "confirm/actions",
  ]);
  assert.deepEqual(world.children("confirm/actions"), [
    "confirm/cancel",
    "confirm/action",
  ]);
  assert.equal(world.skin("confirm/cancel"), "secondary");
  assert.equal(world.skin("confirm/action"), "amber");
  assert.equal(
    world.fields("confirm/action", "GuiButton").get("label"),
    "Delete",
  );
  for (const button of ["confirm/cancel", "confirm/action"]) {
    assert.equal(world.fields(button, "GuiLayout").get("flex"), 1);
    assert.equal(world.fields(button, "GuiLayout").get("height"), 40);
  }
  // Each flexible button gives half the gap from its own share, so the
  // two stay equal.
  assert.equal(
    world.fields("confirm/cancel", "GuiLayout").get("margin_right"),
    8,
  );
  assert.equal(
    world.fields("confirm/action", "GuiLayout").get("margin_left"),
    8,
  );
  assert.deepEqual(themeRows(world, "amber").rows[0]![1], {
    part: PART.background,
    corner_cut: [8, 0, 8, 0],
    border_color: TOKENS.amber,
  });

  // The action confirms once, however many presses arrive.
  await settle(root);
  world.effect("confirm/action", { kind: "pressed" });
  world.effect("confirm/action", { kind: "pressed" });
  world.effect("confirm/cancel", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(answers, ["confirm"]);
  await draw(dialog({ open: false }));
  assert.equal(world.fields("confirm", "GuiBehavior").get("visible"), false);

  // Opened again: the close button cancels.
  await draw(dialog());
  world.effect("confirm/close", { kind: "pressed" });
  await settle(root);
  assert.deepEqual(answers, ["confirm", "cancel"]);
  await draw(dialog({ open: false }));

  // Opened again: Escape, the runtime closing it, cancels once; a late
  // report of the earlier closing does not.
  await draw(dialog());
  world.value("confirm", "GuiBehavior", { visible: false });
  await settle(root);
  assert.deepEqual(answers, ["confirm", "cancel"]);
  world.value("confirm", "GuiBehavior", { visible: true });
  world.value("confirm", "GuiBehavior", { visible: false });
  await settle(root);
  assert.deepEqual(answers, ["confirm", "cancel", "cancel"]);

  // Not destructive: the default primary look.
  await draw(dialog({ destructive: false }));
  assert.equal(world.entity("confirm/action").components.has("GuiSkin"), false);
});

/**
 * A popover holding a trigger whose own menu opens inside it: two overlays,
 * one nested in the other, each with its own open state.
 */
function NestedOverlays({ log }: { readonly log: unknown[] }) {
  const inner = useOverlayOpen({
    defaultOpen: true,
    onOpenChange: (open) => log.push(["inner", open]),
  });
  return h(
    Popover,
    {
      id: "outer",
      label: "Options",
      title: "Options",
      defaultOpen: true,
      onOpenChange: (open) => log.push(["outer", open]),
    },
    h(
      Entity,
      { id: "inner-trigger" },
      h(Button, { label: "More", onPress: inner.toggle }),
      h(
        Children,
        null,
        h(
          Floating,
          {
            id: "inner",
            mode: "light",
            open: inner.open,
            onVisibleChange: inner.onVisibleChange,
          },
          h(Menu, { id: "inner/menu", items: COMMANDS, overlay: inner }),
        ),
      ),
    ),
  );
}

test("closing an overlay nested in another closes only its own open state", async () => {
  const log: unknown[] = [];
  const { world, root } = await render(h(NestedOverlays, { log }));
  await settle(root);
  const visible = (symbol: string) =>
    world.fields(symbol, "GuiBehavior").get("visible");
  assert.equal(visible("outer/popover"), true);
  assert.equal(visible("inner"), true);
  // Both are reported open; the runtime then closes the inner one, as an
  // outside press within the popover does.
  world.value("outer/popover", "GuiBehavior", { visible: true });
  world.value("inner", "GuiBehavior", { visible: true });
  await settle(root);
  world.value("inner", "GuiBehavior", { visible: false });
  await settle(root);
  assert.equal(visible("inner"), false);
  assert.equal(visible("outer/popover"), true);
  assert.deepEqual(log, [["inner", false]]);
  // The outer one's own closing is still adopted.
  world.value("outer/popover", "GuiBehavior", { visible: false });
  await settle(root);
  assert.equal(visible("outer/popover"), false);
  assert.deepEqual(log, [
    ["inner", false],
    ["outer", false],
  ]);
});

/** One open state wired to two overlays, as a mistaken composition would. */
function SharedOpenState({ log }: { readonly log: unknown[] }) {
  const overlay = useOverlayOpen({
    defaultOpen: true,
    onOpenChange: (open) => log.push(open),
  });
  return h(
    Fragment,
    null,
    ["first", "second"].map((id) =>
      h(Floating, {
        key: id,
        id,
        mode: "light",
        open: overlay.open,
        onVisibleChange: overlay.onVisibleChange,
      }),
    ),
  );
}

test("useOverlayOpen adopts a closing only from the entity reported open", async () => {
  const log: unknown[] = [];
  const { world, root } = await render(h(SharedOpenState, { log }));
  await settle(root);
  world.value("first", "GuiBehavior", { visible: true });
  await settle(root);
  // Another entity's closing is ignored...
  world.value("second", "GuiBehavior", { visible: false });
  await settle(root);
  assert.deepEqual(log, []);
  // ...the reported one's is adopted.
  world.value("first", "GuiBehavior", { visible: false });
  await settle(root);
  assert.deepEqual(log, [false]);
});
