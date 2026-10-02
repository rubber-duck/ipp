/**
 * Panels declared through a panel World's generated batch client, for the
 * parts that exercise the runtime's controls, groups and overlays without a
 * kit. Each panel is a `PanelSpec` placed at the origin a part gives it; its
 * controls are `ROW` high in a `PANEL`-unit column unless a panel lays them
 * out itself. Every overlay starts closed, as a kit declares it.
 */
import type { Client, Command } from "@ipp/client";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";
import type { PanelBuild, PanelContext, PanelSpec } from "./page.js";

export const PANEL = 96;
export const ROW = 24;

/**
 * The range slider of the buttons panel: 0..10 between thumbs at 2 and 8,
 * stepping by 1 and by a fine half.
 */
export const RANGE = {
  min: 0,
  max: 10,
  step: 1,
  fine_step: 0.5,
  value: 2,
  upper: 8,
  range: true,
} as const;

/**
 * The groups panel's numeric text input: -4..4 at 1.25, stepping by a
 * quarter and a fine twentieth, shown with two decimals, with its decrement
 * and increment parts, the `ROW`-high squares at its ends.
 */
export const NUMBER = {
  numeric: true,
  value: 1.25,
  min: -4,
  max: 4,
  step: 0.25,
  fine_step: 0.05,
  precision: 2,
  step_parts: true,
} as const;

/** `GuiSlider.axis` of a vertical slider and of a dial. */
const VERTICAL_SLIDER = 1;
const DIAL = 2;

/** `GuiGroup.axis` and `GuiGroup.selection` values. */
const HORIZONTAL = 0;
const VERTICAL = 1;
const SELECT_SINGLE = 1;
const SELECT_FOLLOW = 2;

/**
 * `GuiOverlay.side` below and right of the parent's box and centred on the
 * canvas, and `align` centred and stretched along it.
 */
const OVERLAY_BELOW = 0;
const OVERLAY_RIGHT = 2;
const OVERLAY_CENTRE = 4;
const OVERLAY_CENTRED = 1;
const OVERLAY_STRETCH = 3;

/** `GuiOverlay.mode` values. */
const MODE_LIGHT = 1;
const MODE_MODAL = 2;
const MODE_HINT = 3;

/** `GuiLayout.kind` values. */
const LEAF = 0;
const ROW_LAYOUT = 1;
const COLUMN = 2;

type Fields = Record<string, number | string | boolean>;

/**
 * One panel's commands, aliases from 1, with each entity's symbolic id
 * `composite-<name>`; `named` entities are the ones cases address.
 */
class PanelCommands {
  readonly commands: Command[] = [];
  readonly #named: Record<string, number> = {};
  #next = 1;

  constructor(private readonly client: Client) {}

  add(
    name: string,
    parent: number | null,
    components: readonly (readonly [string, Fields])[],
    named = false,
  ) {
    const id = this.#next++;
    this.commands.push(createEntity(id, `composite-${name}`));
    for (const [component, fields] of components)
      this.commands.push(
        insertComponent(this.client, component, alias(id), fields),
      );
    if (parent !== null)
      this.commands.push({
        kind: "placeEntity",
        entity: alias(id),
        placement: { parent: alias(parent), before: null },
      });
    if (named) this.#named[name] = id;
    return id;
  }

  /** A named control: its component, a leaf box and optional behavior. */
  control(
    name: string,
    parent: number,
    component: string,
    fields: Fields,
    width: number,
    height = ROW,
    behavior?: Fields,
  ) {
    return this.add(
      name,
      parent,
      [
        [component, fields],
        box(LEAF, width, height),
        ...(behavior ? [["GuiBehavior", behavior] as const] : []),
      ],
      true,
    );
  }

  /** Apply the commands; returns the named entities. */
  async apply(): Promise<Record<string, bigint>> {
    const created = successfulBatch(await this.client.batch(this.commands));
    return Object.fromEntries(
      Object.entries(this.#named).map(([name, id]) => [
        name,
        aliasId(created, id),
      ]),
    );
  }
}

const alias = (value: number) => ({ kind: "alias" as const, alias: value });

const box = (kind: number, width: number, height: number) =>
  ["GuiLayout", { kind, width, height }] as const;

/** A panel's root: a box of `kind` filling it, with the shared font. */
const root = (
  panel: PanelCommands,
  name: string,
  { font }: PanelContext,
  kind: number,
  [width, height]: readonly [number, number],
) =>
  panel.add(name, null, [
    box(kind, width, height),
    ["GuiFont", { source: font, font_size: 16 }],
  ]);

/** A panel declared by commands at `origin`. */
function commandPanel(
  name: string,
  origin: readonly [number, number],
  extent: readonly [number, number],
  declare: (panel: PanelCommands, context: PanelContext) => void,
): PanelSpec {
  return {
    name,
    origin,
    extent,
    async build(context): Promise<PanelBuild> {
      const panel = new PanelCommands(context.client);
      declare(panel, context);
      return { entities: await panel.apply() };
    },
  };
}

/**
 * Buttons `a` and `b`, the text input `text` holding "ab" and `range`, the
 * range slider `RANGE`, in a `PANEL`-square column.
 */
export function buttonsPanel(origin: readonly [number, number]) {
  return commandPanel("buttons", origin, [PANEL, PANEL], (panel, context) => {
    const column = root(panel, "buttons-root", context, COLUMN, [PANEL, PANEL]);
    panel.control("a", column, "GuiButton", {}, PANEL);
    panel.control("b", column, "GuiButton", {}, PANEL);
    panel.control("text", column, "GuiTextInput", { text: "ab" }, PANEL);
    panel.control("range", column, "GuiSlider", RANGE, PANEL);
  });
}

/**
 * Button `c`, the disabled button `d` and a `2 x ROW` row of sliders in a
 * `PANEL`-square column: `vertical`, a `24 x 48` vertical slider over 0..10
 * at 5, beside `scroll`, a `72 x 48` scroll view whose `72 x 96` content
 * starts with `slider`, a `72 x 24` horizontal slider over 0..10 at 5. Both
 * step by 1, with Shift by a fine quarter and a fine half.
 */
export function slidersPanel(origin: readonly [number, number]) {
  return commandPanel("sliders", origin, [PANEL, PANEL], (panel, context) => {
    const column = root(panel, "sliders-root", context, COLUMN, [PANEL, PANEL]);
    panel.control("c", column, "GuiButton", {}, PANEL);
    panel.control("d", column, "GuiButton", {}, PANEL, ROW, { enabled: false });
    const steps = { min: 0, max: 10, step: 1, value: 5 };
    const row = panel.add("slider-row", column, [
      box(ROW_LAYOUT, PANEL, 2 * ROW),
    ]);
    panel.control(
      "vertical",
      row,
      "GuiSlider",
      { ...steps, fine_step: 0.25, axis: VERTICAL_SLIDER },
      ROW,
      2 * ROW,
    );
    const scroll = panel.add(
      "scroll",
      row,
      [["GuiScrollView", {}], box(LEAF, PANEL - ROW, 2 * ROW)],
      true,
    );
    const content = panel.add("scroll-content", scroll, [
      box(COLUMN, PANEL - ROW, 4 * ROW),
    ]);
    panel.control(
      "slider",
      content,
      "GuiSlider",
      { ...steps, fine_step: 0.5 },
      PANEL - ROW,
    );
    panel.add("scroll-filler", content, [box(LEAF, PANEL - ROW, 3 * ROW)]);
  });
}

/** The dial's scroll content's spacer and filler; the dial is twice as high. */
const DIAL_SPACE = 32;

/**
 * `dial-scroll`, a `PANEL`-square scroll view whose `96 x 128` column holds
 * a `96 x 32` spacer, `dial`, a `64 x 64` dial over 0..10 at 5 that steps by
 * 1 and by a fine half, and a `96 x 32` filler, so the dial stays whole in
 * view over the 32 units of travel.
 */
export function dialPanel(origin: readonly [number, number]) {
  return commandPanel("dial", origin, [PANEL, PANEL], (panel, context) => {
    const column = root(panel, "dial-root", context, COLUMN, [PANEL, PANEL]);
    const scroll = panel.add(
      "dial-scroll",
      column,
      [["GuiScrollView", {}], box(LEAF, PANEL, PANEL)],
      true,
    );
    const content = panel.add("dial-content", scroll, [
      box(COLUMN, PANEL, 4 * DIAL_SPACE),
    ]);
    panel.add("dial-spacer", content, [box(LEAF, PANEL, DIAL_SPACE)]);
    panel.control(
      "dial",
      content,
      "GuiSlider",
      { min: 0, max: 10, step: 1, fine_step: 0.5, value: 5, axis: DIAL },
      2 * DIAL_SPACE,
      2 * DIAL_SPACE,
    );
    panel.add("dial-filler", content, [box(LEAF, PANEL, DIAL_SPACE)]);
  });
}

/**
 * A row of two `PANEL`-square columns. The left column stacks the segmented
 * group `seg-x`, `seg-y`, `seg-z` (`seg-y` selected) whose arrows select, the
 * tab group `tab-1`, `tab-2`, `tab-3` (`tab-1` selected) whose arrows move
 * focus and whose activation selects, and the button `after`. The right
 * column holds `query`, a text input holding "ab" whose light overlay
 * `options` below it is a single-selection list of rows `opt-1` to `opt-3`
 * that do not take focus, and below it `number`, the numeric input `NUMBER`.
 */
export function groupsPanel(origin: readonly [number, number]) {
  return commandPanel(
    "groups",
    origin,
    [2 * PANEL, PANEL],
    (panel, context) => {
      const row = root(panel, "groups-root", context, ROW_LAYOUT, [
        2 * PANEL,
        PANEL,
      ]);
      const left = panel.add("groups-left", row, [box(COLUMN, PANEL, PANEL)]);
      const segmented = panel.add("segmented", left, [
        box(ROW_LAYOUT, PANEL, ROW),
        ["GuiGroup", { axis: HORIZONTAL, selection: SELECT_FOLLOW }],
      ]);
      for (const name of ["seg-x", "seg-y", "seg-z"])
        panel.control(
          name,
          segmented,
          "GuiButton",
          { label: name.slice(4).toUpperCase(), selected: name === "seg-y" },
          PANEL / 3,
        );
      const tabs = panel.add("tabs", left, [
        box(ROW_LAYOUT, PANEL, ROW),
        ["GuiGroup", { axis: HORIZONTAL, selection: SELECT_SINGLE }],
      ]);
      for (const name of ["tab-1", "tab-2", "tab-3"])
        panel.control(
          name,
          tabs,
          "GuiButton",
          { label: name.slice(4), selected: name === "tab-1" },
          PANEL / 3,
        );
      panel.control("after", left, "GuiButton", { label: "OK" }, PANEL);
      const right = panel.add("groups-right", row, [box(COLUMN, PANEL, PANEL)]);
      const query = panel.control(
        "query",
        right,
        "GuiTextInput",
        { text: "ab" },
        PANEL,
      );
      const options = panel.add(
        "options",
        query,
        [
          box(COLUMN, PANEL, 3 * ROW),
          ["GuiGroup", { axis: VERTICAL, selection: SELECT_SINGLE }],
          [
            "GuiOverlay",
            { side: OVERLAY_BELOW, align: OVERLAY_STRETCH, mode: MODE_LIGHT },
          ],
          ["CanvasStyle", { layer: 1 }],
          ["GuiBehavior", { visible: false }],
        ],
        true,
      );
      for (const name of ["opt-1", "opt-2", "opt-3"])
        panel.control(
          name,
          options,
          "GuiButton",
          { label: name.toUpperCase() },
          PANEL,
          ROW,
          { focusable: false },
        );
      panel.control("number", right, "GuiTextInput", NUMBER, PANEL);
    },
  );
}

/**
 * A row of two `PANEL`-square columns, the right one empty. The left column
 * stacks:
 *
 * - `menu`, a dropdown trigger whose light list `menu-list` below it,
 *   stretched to its width, holds a single-selection group of three rows
 *   `item-1` to `item-3` that take no focus and covers the controls under it;
 * - `beneath`, which the open list and the dialog cover;
 * - `help`, `ROW` square, whose hint `tip` opens to its right;
 * - `opener`, whose light popover `pop` opens to its right with `pop-ok` and
 *   `pop-cancel`.
 *
 * `dialog` is a top-level modal overlay centred on the canvas, `96 x 48`,
 * with `dialog-no` and `dialog-yes`.
 */
export function overlaysPanel(origin: readonly [number, number]) {
  return commandPanel(
    "overlays",
    origin,
    [2 * PANEL, PANEL],
    (panel, context) => {
      const row = root(panel, "overlays-root", context, ROW_LAYOUT, [
        2 * PANEL,
        PANEL,
      ]);
      const left = panel.add("overlays-left", row, [box(COLUMN, PANEL, PANEL)]);
      panel.add("overlays-right", row, [box(LEAF, PANEL, PANEL)]);
      const button = (
        name: string,
        parent: number,
        width: number,
        label: string,
        focusable = true,
      ) =>
        panel.control(name, parent, "GuiButton", { label }, width, ROW, {
          focusable,
        });
      const overlay = (
        name: string,
        parent: number | null,
        fields: Fields,
        layer: number,
        [width, height]: readonly [number, number],
      ) =>
        panel.add(
          name,
          parent,
          [
            ["GuiOverlay", fields],
            ["CanvasStyle", { layer }],
            ["GuiBehavior", { visible: false }],
            box(COLUMN, width, height),
          ],
          true,
        );
      const menu = button("menu", left, PANEL, "MENU");
      const list = overlay(
        "menu-list",
        menu,
        { side: OVERLAY_BELOW, align: OVERLAY_STRETCH, mode: MODE_LIGHT },
        1,
        [PANEL, 3 * ROW],
      );
      panel.commands.push(
        insertComponent(context.client, "GuiGroup", alias(list), {
          axis: VERTICAL,
          selection: SELECT_SINGLE,
        }),
      );
      for (const name of ["item-1", "item-2", "item-3"])
        button(name, list, PANEL, name.toUpperCase(), false);
      button("beneath", left, PANEL, "UNDER");
      const helpRow = panel.add("help-row", left, [
        box(ROW_LAYOUT, PANEL, ROW),
      ]);
      const help = button("help", helpRow, ROW, "?");
      const tip = overlay(
        "tip",
        help,
        { side: OVERLAY_RIGHT, mode: MODE_HINT },
        1,
        [PANEL - ROW, ROW],
      );
      button("tip-text", tip, PANEL - ROW, "TIP", false);
      const opener = button("opener", left, PANEL, "OPEN");
      const pop = overlay(
        "pop",
        opener,
        { side: OVERLAY_RIGHT, mode: MODE_LIGHT },
        1,
        [PANEL, 2 * ROW],
      );
      button("pop-ok", pop, PANEL, "OK");
      button("pop-cancel", pop, PANEL, "NO");
      const dialog = overlay(
        "dialog",
        null,
        { side: OVERLAY_CENTRE, align: OVERLAY_CENTRED, mode: MODE_MODAL },
        2,
        [PANEL, 2 * ROW],
      );

      // A top-level overlay inherits nothing from the panel's root.
      panel.commands.push(
        insertComponent(context.client, "GuiFont", alias(dialog), {
          source: context.font,
          font_size: 16,
        }),
      );
      button("dialog-no", dialog, PANEL, "NO");
      button("dialog-yes", dialog, PANEL, "YES");
    },
  );
}
