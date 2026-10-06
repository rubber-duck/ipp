/**
 * Sheet f, the searchable dropdown, drawn with the GUI kit's
 * `SearchableDropdown`: closed with Alpha Station selected; opened by a real
 * click, which moves focus into its search field, and searched for "al",
 * leaving the selected Alpha Station; searched for "zz", showing the empty
 * row; and, below the crop, a list still loading options after the one it
 * has.
 *
 * The kit keeps the trigger in place, showing the selection while the
 * search field and results open below it, where the sheet draws the field
 * over the trigger. Each open state is pinned on its own, since focus
 * entering one list closes any other. Drawn at the selection sheet's scale
 * through a nested `GuiKit`.
 */
import {
  GuiKit,
  SearchableDropdown,
  type SearchableDropdownProps,
  type SelectOption,
} from "@ipp/react/gui-kit";
import {
  Fill,
  Label,
  SHEET_CAPTION,
  SHEET_PAGE,
  around,
  centre,
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

/** The f02 crop is 2x of sheet f (776, 110) to (1526, 514); a list added below. */
const EXTENT = [750, 580] as const;

const u = (value: number) => value * K;

/**
 * A pinned cell's margin: the frame glow's reach at the sheet's scale, so the
 * focused field's glow ends inside the cell pasted from its capture.
 */
const GLOW = u(20);

const OPTIONS: readonly SelectOption[] = [
  { key: "alpha", label: "Alpha Station" },
  { key: "beta", label: "Beta Relay" },
  { key: "gamma", label: "Gamma Dock" },
  { key: "delta", label: "Delta Array" },
];

/** Each dropdown's top-left, width, rows it shows open, props and search. */
const DROPDOWNS = {
  closed: { at: [32, 102], width: u(210), rows: 0, props: {} },
  results: { at: [340, 82], width: u(216), rows: 1, props: {}, search: "al" },
  empty: { at: [340, 320], width: u(216), rows: 1, props: {}, search: "zz" },
  loading: {
    at: [32, 300],
    width: u(210),
    rows: 2,
    props: { loading: "Loading…" },
    search: "be",
  },
} as const satisfies Record<
  string,
  {
    at: Point;
    width: number;
    rows: number;
    props: Partial<SearchableDropdownProps>;
    search?: string;
  }
>;
type Name = keyof typeof DROPDOWNS;

const HEIGHT = u(CONTROL_HEIGHT);

/** The trigger's box. */
const trigger = (name: Name): Rect => [
  ...DROPDOWNS[name].at,
  DROPDOWNS[name].width,
  HEIGHT,
];

/**
 * The trigger with its open surface: a quarter inset below it, the search
 * field and the rows in their half-inset margins inside the surface's line.
 */
const open = (name: Name): Rect => [
  ...DROPDOWNS[name].at,
  DROPDOWNS[name].width,
  HEIGHT +
    u(INSET / 4 + 2 * LINE + INSET / 2 + CONTROL_HEIGHT) +
    u(DROPDOWNS[name].rows * ROW + INSET),
];

const CAPTIONS: Readonly<Record<Name, string>> = {
  closed: "Closed",
  results: "Open (search results)",
  empty: "Open (no results)",
  loading: "Loading",
};

/**
 * Click the trigger, wait for the list it opens, which takes focus, and
 * type the search.
 */
const search = (name: Name, text: string) => [
  { kind: "click" as const, at: centre(trigger(name)) },
  { kind: "settle" as const },
  {
    kind: "action" as const,
    control: name,
    action: { kind: "text" as const, value: text },
  },
];

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "f02-searchable-dropdown.png", origin: [0, 0] },
  states: [
    { name: "closed", cell: around(trigger("closed"), 12) },
    ...(["results", "empty", "loading"] as const).map((name) => ({
      name,
      cell: around(open(name), GLOW),
      pin: search(name, DROPDOWNS[name].search),
    })),
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      {(Object.keys(DROPDOWNS) as Name[]).map((name) => (
        // The loading row's spinner stands still, so paint settles.
        <GuiKit
          key={name}
          fontSize={u(TEXT_BODY)}
          reducedMotion={name === "loading"}
        >
          <SearchableDropdown
            id={`search-${name}`}
            label="Node"
            options={OPTIONS}
            defaultValue="alpha"
            {...DROPDOWNS[name].props}
            searchRef={lab.control(name)}
            layout={placed(DROPDOWNS[name].at, DROPDOWNS[name].width)}
          />
        </GuiKit>
      ))}
      {(Object.keys(CAPTIONS) as Name[]).map((name) => {
        const [x, y, , height] = name === "closed" ? trigger(name) : open(name);
        return (
          <Label
            key={name}
            id={`caption-${name}`}
            at={[x, y + height + 8]}
            text={CAPTIONS[name]}
            font={lab.font}
            size={u(12.5)}
            color={SHEET_CAPTION}
          />
        );
      })}
    </>
  ),
});
