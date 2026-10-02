/**
 * Sheet g, the context menu, drawn with the GUI kit's `ContextMenu` and
 * opened through real input: a click focuses a target, the Menu key requests
 * its context at the bottom-left corner of its box, and the menu opens there.
 * The sheet's commands with Inspect active under the pointer, a separator
 * and the destructive Delete in amber; beside them the sheet's unavailable
 * command, disabled; below the crop, a menu whose active row two Downs moved
 * while focus stays on the target. Each target sits just above where the
 * sheet draws its menu, and each state opens its own menu, which closes when
 * the state's input is released.
 *
 * The kit draws its rows at the language's row height and its frame with the
 * paired cut, so the menu is shorter and less cut than the sheet's. Drawn at
 * the context sheet's scale through a nested `GuiKit`.
 */
import { Entity, type AssetReference } from "@ipp/react";
import { Button, Font, Layout } from "@ipp/react/gui";
import {
  ContextMenu,
  GuiKit,
  MENU_MIN_WIDTH,
  useContextMenu,
  type MenuItem,
} from "@ipp/react/gui-kit";
import { Fill, Label, SHEET_CAPTION, SHEET_PAGE, around } from "../kit.js";
import { SHEET_G_SCALE as K } from "../scale.js";
import {
  defineSpecimen,
  type PinStep,
  type Point,
  type Rect,
} from "../specimen.js";
import {
  CONTROL_HEIGHT,
  INSET,
  LINE,
  ROW,
  TEXT_BODY,
} from "../themes/geometry.js";

/** The g02 crop is 2x of sheet g; a menu driven by keys added below. */
const EXTENT = [752, 720] as const;

const u = (value: number) => value * K;

/** Material outline glyphs of the shared font. */
const GLYPH = {
  file: "\u{f0224}",
  copy: "\u{f018f}",
  trash: "\u{f0a7a}",
} as const;

const COMMANDS: readonly MenuItem[] = [
  { key: "inspect", label: "Inspect", icon: GLYPH.file },
  { key: "duplicate", label: "Duplicate", icon: GLYPH.copy },
  {
    key: "delete",
    label: "Delete",
    icon: GLYPH.trash,
    tone: "amber",
    separator: true,
  },
];

const UNAVAILABLE: readonly MenuItem[] = [
  {
    key: "unavailable",
    label: "Unavailable",
    icon: GLYPH.file,
    disabled: true,
  },
];

/**
 * Each target's box, its bottom-left corner where the sheet's menu frame
 * starts: (135, 123.5) and (460, 209) in the crop.
 */
const TARGETS = {
  active: [135, 123.5 - u(CONTROL_HEIGHT), u(120), u(CONTROL_HEIGHT)],
  disabled: [460, 209 - u(CONTROL_HEIGHT), u(120), u(CONTROL_HEIGHT)],
  keyboard: [135, 390, u(120), u(CONTROL_HEIGHT)],
} as const satisfies Record<string, Rect>;
type Target = keyof typeof TARGETS;

const ITEMS: Readonly<Record<Target, readonly MenuItem[]>> = {
  active: COMMANDS,
  disabled: UNAVAILABLE,
  keyboard: [...COMMANDS, ...UNAVAILABLE],
};

/** The kit menu's extent: its narrowest width, rows and frame line. */
const WIDTH = u(MENU_MIN_WIDTH + 2 * LINE);
const height = (items: readonly MenuItem[]) =>
  u(
    INSET +
      items.length * ROW +
      items.filter((item, index) => item.separator && index > 0).length *
        (LINE + INSET / 2) +
      2 * LINE,
  );

/** Where a target's menu opens: its box's bottom-left corner. */
const opens = (target: Target): Point => [
  TARGETS[target][0],
  TARGETS[target][1] + TARGETS[target][3],
];

/** The centre of row `index` of a menu whose frame starts at `at`. */
const row = (at: Point, index: number): Point => [
  at[0] + WIDTH / 2,
  at[1] + u(LINE + INSET / 2 + (index + 0.5) * ROW),
];

/** A state's cell: its target and its menu with their glow. */
const cell = (target: Target): Rect =>
  around(
    [
      TARGETS[target][0],
      TARGETS[target][1],
      WIDTH,
      TARGETS[target][3] + height(ITEMS[target]),
    ],
    16,
  );

/** Click the target, request its context from the keyboard and let it open. */
const open = (target: Target): PinStep[] => [
  {
    kind: "click",
    at: [
      TARGETS[target][0] + TARGETS[target][2] / 2,
      TARGETS[target][1] + TARGETS[target][3] / 2,
    ],
  },
  { kind: "key", key: "contextMenu" },
  { kind: "wait", seconds: 0.25 },
];

/** A target and the context menu its request opens. */
function ContextTarget({
  name,
  font,
}: {
  readonly name: Target;
  readonly font: AssetReference;
}) {
  const menu = useContextMenu<Target>();
  const [x, y, width, height] = TARGETS[name];
  return (
    <>
      <Entity id={`target-${name}`}>
        <Layout
          kind={0}
          width={width}
          height={height}
          margin_left={x}
          margin_top={y}
          align_x={-1}
          align_y={-1}
        />
        <Font source={font} font_size={u(TEXT_BODY)} />
        <Button label="Cube" onContextMenu={menu.opener(name)} />
      </Entity>
      <Entity id={`menu-host-${name}`}>
        <ContextMenu id={`menu-${name}`} menu={menu} items={ITEMS[name]} />
      </Entity>
    </>
  );
}

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "g02-context-menu.png", origin: [0, 0] },
  states: [
    {
      name: "active",
      cell: cell("active"),
      pin: [...open("active"), { kind: "hover", at: row(opens("active"), 0) }],
    },
    { name: "disabled", cell: cell("disabled"), pin: open("disabled") },
    {
      name: "keyboard",
      cell: cell("keyboard"),
      pin: [
        ...open("keyboard"),
        { kind: "key", key: "down" },
        { kind: "key", key: "down" },
      ],
    },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      {(
        [
          ["Outside press closes", [135, 343]],
          ["Escape dismisses", [461, 343]],
        ] as const
      ).map(([text, at], index) => (
        <Label
          key={text}
          id={`caption-${index}`}
          at={at}
          text={text}
          font={lab.font}
          size={u(12.5)}
          color={SHEET_CAPTION}
        />
      ))}
      <GuiKit fontSize={u(TEXT_BODY)}>
        {(Object.keys(TARGETS) as Target[]).map((name) => (
          <ContextTarget key={name} name={name} font={lab.font} />
        ))}
      </GuiKit>
    </>
  ),
});
