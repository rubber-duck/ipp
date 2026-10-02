/**
 * Sheet f, the multi-select, drawn with the GUI kit's `MultiSelect`: closed
 * with Render and Physics selected, whose labels fit the trigger; closed
 * with every channel selected in a narrower trigger, where they do not and
 * the count stands in for them; and open, where the selected channels carry
 * the check mark and the pointer makes Physics the active option.
 *
 * The kit summarises the selection in the trigger's text instead of the
 * sheet's chips with remove marks, and draws the selection as the check mark
 * rather than a box on every row. The open list hangs below its trigger.
 * Drawn at the selection sheet's scale through a nested `GuiKit`.
 */
import {
  GuiKit,
  MultiSelect,
  type MultiSelectProps,
  type SelectOption,
} from "@ipp/react/gui-kit";
import {
  Fill,
  Label,
  SHEET_CAPTION,
  SHEET_PAGE,
  around,
  placed,
} from "../kit.js";
import { SHEET_F_SCALE as K } from "../scale.js";
import { defineSpecimen, type Point, type Rect } from "../specimen.js";
import {
  CONTROL_HEIGHT,
  INSET,
  LINE,
  ROW,
  TEXT_BODY,
} from "../themes/geometry.js";

/** The f03 crop is 2x of sheet f (12, 522) to (762, 938). */
const EXTENT = [750, 416] as const;

const u = (value: number) => value * K;

/**
 * A pinned cell's margin: the frame glow's reach at the sheet's scale, so the
 * active row's glow ends inside the cell pasted from its capture.
 */
const GLOW = u(20);

const CHANNELS: readonly SelectOption[] = [
  { key: "render", label: "Render" },
  { key: "physics", label: "Physics" },
  { key: "network", label: "Network" },
];

/** Each multi-select's top-left, width and props, in tree order. */
const SELECTS = {
  closed: {
    at: [35, 107],
    width: u(296),
    props: { defaultValue: ["render", "physics"] },
  },
  overflow: {
    at: [35, 275],
    width: u(252),
    props: { defaultValue: ["render", "physics", "network"] },
  },
  open: {
    at: [430, 30],
    width: u(240),
    props: { defaultValue: ["render", "physics"], defaultOpen: true },
  },
} as const satisfies Record<
  string,
  { at: Point; width: number; props: Partial<MultiSelectProps> }
>;
type Name = keyof typeof SELECTS;

const HEIGHT = u(CONTROL_HEIGHT);

const trigger = (name: Name): Rect => [
  ...SELECTS[name].at,
  SELECTS[name].width,
  HEIGHT,
];

/** The open list's top: a quarter inset below its trigger, inside its line. */
const LIST_TOP = SELECTS.open.at[1] + HEIGHT + u(INSET / 4 + LINE);
const LIST_HEIGHT = u(CHANNELS.length * ROW + INSET + 2 * LINE);

/** The centre of option `index` of the open list. */
const option = (index: number): Point => [
  SELECTS.open.at[0] + SELECTS.open.width / 2,
  LIST_TOP + u(INSET / 2 + (index + 0.5) * ROW),
];

const OPEN: Rect = [
  ...SELECTS.open.at,
  SELECTS.open.width,
  LIST_TOP - SELECTS.open.at[1] + LIST_HEIGHT,
];

const CAPTIONS: Readonly<Record<Name, [string, Rect]>> = {
  closed: ["Closed", trigger("closed")],
  overflow: ["Selection overflow", trigger("overflow")],
  open: ["Open", OPEN],
};

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "f03-multi-select.png", origin: [0, 0] },
  states: [
    { name: "closed", cell: around(trigger("closed"), 12) },
    { name: "overflow", cell: around(trigger("overflow"), 12) },
    {
      // The pointer over Physics makes it the active option.
      name: "open",
      cell: around(OPEN, GLOW),
      pin: [{ kind: "hover", at: option(1) }],
    },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <GuiKit fontSize={u(TEXT_BODY)}>
        {(Object.keys(SELECTS) as Name[]).map((name) => (
          <MultiSelect
            key={name}
            id={`multi-${name}`}
            label="Channels"
            options={CHANNELS}
            {...SELECTS[name].props}
            layout={placed(SELECTS[name].at, SELECTS[name].width)}
          />
        ))}
      </GuiKit>
      {(Object.keys(CAPTIONS) as Name[]).map((name) => {
        const [text, [x, y, , height]] = CAPTIONS[name];
        return (
          <Label
            key={name}
            id={`caption-${name}`}
            at={[x, y + height + 8]}
            text={text}
            font={lab.font}
            size={u(12.5)}
            color={SHEET_CAPTION}
          />
        );
      })}
    </>
  ),
});
