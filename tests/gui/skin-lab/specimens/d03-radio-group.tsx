/**
 * Sheet d, the radio group, drawn with the GUI kit's `RadioGroup`: the
 * sheet's AXIS group with X selected and Z disabled, its marks where the
 * sheet has them; then, below the crop, the states the sheet omits, each on
 * a group of its own: a hovered mark, keyboard focus on the selected mark,
 * Right moving focus and selection together, a pressed mark and a disabled
 * selected option.
 *
 * The kit spaces options an inset apart, where the sheet spreads them over
 * its width. Drawn at the value sheet's scale through a nested `GuiKit`.
 */
import { GuiKit, RadioGroup } from "@ipp/react/gui-kit";
import {
  FONT_ADVANCE,
  Fill,
  Label,
  SHEET_CAPTION,
  SHEET_PAGE,
  around,
  placed,
} from "../kit.js";
import { SHEET_D_SCALE as K } from "../scale.js";
import { defineSpecimen, type Point, type Rect } from "../specimen.js";
import {
  DENSE_ROW,
  ICON,
  INSET,
  SMALL_HEIGHT,
  TEXT_BODY,
} from "../themes/geometry.js";

/** The d03 crop is 2x of sheet d (1022, 90) to (1522, 288); groups added below. */
const EXTENT = [500, 600] as const;

const u = (value: number) => value * K;
const BODY = u(TEXT_BODY);

const OPTIONS = [
  { value: "x", label: "X" },
  { value: "y", label: "Y" },
  { value: "z", label: "Z", disabled: true },
] as const;

/** An option's width: its mark, the gap and its label. */
const OPTION = u(ICON + INSET / 2) + FONT_ADVANCE * BODY + BODY / 100;
const SPACING = u(INSET);
const WIDTH = OPTIONS.length * OPTION + (OPTIONS.length - 1) * SPACING;

/** Each group's top-left; the sheet's has the caption above its marks. */
const GROUPS = {
  sheet: [61.2, 69],
  hover: [40, 236],
  focus: [264, 236],
  arrow: [40, 356],
  pressed: [264, 356],
  disabled: [40, 476],
} as const satisfies Record<string, Point>;
type Group = keyof typeof GROUPS;

const caption = (group: Group) => (group === "sheet" ? u(DENSE_ROW) : 0);

/** A group's outer rectangle. */
const rect = (group: Group): Rect => [
  ...GROUPS[group],
  WIDTH,
  caption(group) + u(SMALL_HEIGHT),
];

/** The centre of option `index`'s mark in `group`. */
const mark = (group: Group, index: number): Point => [
  GROUPS[group][0] + index * (OPTION + SPACING) + u(ICON) / 2,
  GROUPS[group][1] + caption(group) + u(SMALL_HEIGHT) / 2,
];

const CAPTIONS: Readonly<Record<Exclude<Group, "sheet">, string>> = {
  hover: "Hover",
  focus: "Keyboard focus",
  arrow: "Right moves and selects",
  pressed: "Pressed",
  disabled: "Disabled selected",
};

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "d03-radio-group.png", origin: [0, 0] },
  states: [
    { name: "sheet", cell: around(rect("sheet"), 12) },
    {
      name: "hover",
      cell: around(rect("hover"), 12),
      pin: [{ kind: "hover", at: mark("hover", 1) }],
    },
    {
      // A press focuses X without the ring; Home lights it as the keyboard
      // target, and it stays selected.
      name: "focus",
      cell: around(rect("focus"), 12),
      pin: [
        { kind: "click", at: mark("focus", 0) },
        { kind: "key", key: "home" },
      ],
    },
    {
      // The press on X selects it again, so the pin repeats.
      name: "arrow",
      cell: around(rect("arrow"), 12),
      pin: [
        { kind: "click", at: mark("arrow", 0) },
        { kind: "key", key: "right" },
      ],
    },
    {
      name: "pressed",
      cell: around(rect("pressed"), 12),
      pin: [{ kind: "press", at: mark("pressed", 1) }],
    },
    { name: "disabled", cell: around(rect("disabled"), 12) },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <GuiKit fontSize={BODY}>
        {(Object.keys(GROUPS) as Group[]).map((group) => (
          <RadioGroup
            key={group}
            id={`radio-${group}`}
            {...(group === "sheet" ? { label: "AXIS" } : {})}
            options={OPTIONS}
            defaultValue={group === "disabled" ? "z" : "x"}
            horizontal
            layout={placed(GROUPS[group], WIDTH)}
          />
        ))}
      </GuiKit>
      {(Object.keys(CAPTIONS) as Exclude<Group, "sheet">[]).map((group) => (
        <Label
          key={group}
          id={`caption-${group}`}
          at={[GROUPS[group][0], GROUPS[group][1] + u(SMALL_HEIGHT) + 6]}
          text={CAPTIONS[group]}
          font={lab.font}
          size={u(12.5)}
          color={SHEET_CAPTION}
        />
      ))}
    </>
  ),
});
