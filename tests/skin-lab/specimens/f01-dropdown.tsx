/**
 * Sheet f, the dropdown, drawn with the GUI kit's `Dropdown`: closed with
 * Aurora selected, and open, where Aurora shows the selected row's bar and
 * tint and the pointer makes Neon the active option; the list adds a
 * disabled option, which keeps its label readable in the neutral tone.
 * Below the closed trigger, where the sheet has room, a trigger without a
 * selection shows its placeholder and a disabled one its neutral value.
 *
 * The open dropdown is declared open, as an application opening it would.
 * The list hangs a quarter inset below its trigger rather than attached as
 * the sheet draws it, so the paired cut never notches the joint. Drawn at
 * the selection sheet's scale through a nested `GuiKit`.
 */
import {
  Dropdown,
  GuiKit,
  type DropdownProps,
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

/** The f01 crop is 2x of sheet f (12, 110) to (762, 514). */
const EXTENT = [750, 404] as const;

const u = (value: number) => value * K;

/**
 * A pinned cell's margin: the frame glow's reach at the sheet's scale, so a
 * glow its pins light ends inside the cell pasted from their capture.
 */
const GLOW = u(20);

const OPTIONS: readonly SelectOption[] = [
  { key: "aurora", label: "Aurora" },
  { key: "ember", label: "Ember" },
  { key: "neon", label: "Neon" },
  { key: "static", label: "Static", disabled: true },
];

/** Each trigger's top-left, width and props, in tree order. */
const TRIGGERS = {
  closed: { at: [35, 103], width: u(222), props: { defaultValue: "aurora" } },
  open: {
    at: [343, 100],
    width: u(196),
    props: { defaultValue: "aurora", defaultOpen: true },
  },
  placeholder: {
    at: [35, 192],
    width: u(222),
    props: { placeholder: "Select skin" },
  },
  disabled: {
    at: [35, 272],
    width: u(222),
    props: { defaultValue: "ember", disabled: true },
  },
} as const satisfies Record<
  string,
  { at: Point; width: number; props: Partial<DropdownProps> }
>;
type Trigger = keyof typeof TRIGGERS;

const HEIGHT = u(CONTROL_HEIGHT);

/** The trigger's box. */
const box = (trigger: Trigger): Rect => [
  ...TRIGGERS[trigger].at,
  TRIGGERS[trigger].width,
  HEIGHT,
];

/** The open list's top: a quarter inset below its trigger, inside its line. */
const LIST_TOP = TRIGGERS.open.at[1] + HEIGHT + u(INSET / 4 + LINE);

/** The list's height: its rows inside a half-inset margin and its line. */
const LIST_HEIGHT = u(OPTIONS.length * ROW + INSET + 2 * LINE);

/** The centre of option `index` of the open list. */
const option = (index: number): Point => [
  TRIGGERS.open.at[0] + TRIGGERS.open.width / 2,
  LIST_TOP + u(INSET / 2 + (index + 0.5) * ROW),
];

const CAPTIONS: Readonly<Record<Trigger, [string, number]>> = {
  closed: ["Closed", HEIGHT + 8],
  open: ["Open", LIST_TOP - TRIGGERS.open.at[1] + LIST_HEIGHT + 8],
  placeholder: ["Placeholder", HEIGHT + 8],
  disabled: ["Disabled", HEIGHT + 8],
};

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "f01-dropdown.png", origin: [0, 0] },
  states: [
    { name: "closed", cell: around(box("closed"), 12) },
    {
      // The pointer over Neon makes it the active option.
      name: "open",
      cell: around(
        [
          ...TRIGGERS.open.at,
          TRIGGERS.open.width,
          LIST_TOP + LIST_HEIGHT - TRIGGERS.open.at[1],
        ],
        GLOW,
      ),
      pin: [{ kind: "hover", at: option(2) }],
    },
    { name: "placeholder", cell: around(box("placeholder"), 12) },
    { name: "disabled", cell: around(box("disabled"), 12) },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <GuiKit fontSize={u(TEXT_BODY)}>
        {(Object.keys(TRIGGERS) as Trigger[]).map((trigger) => (
          <Dropdown
            key={trigger}
            id={`dropdown-${trigger}`}
            label="Skin"
            options={OPTIONS}
            {...TRIGGERS[trigger].props}
            layout={placed(TRIGGERS[trigger].at, TRIGGERS[trigger].width)}
          />
        ))}
      </GuiKit>
      {(Object.keys(CAPTIONS) as Trigger[]).map((trigger) => (
        <Label
          key={trigger}
          id={`caption-${trigger}`}
          at={[
            TRIGGERS[trigger].at[0],
            TRIGGERS[trigger].at[1] + CAPTIONS[trigger][1],
          ]}
          text={CAPTIONS[trigger][0]}
          font={lab.font}
          size={u(12.5)}
          color={SHEET_CAPTION}
        />
      ))}
    </>
  ),
});
