/**
 * Sheet f, the autocomplete, drawn with the GUI kit's `Autocomplete`: a
 * field focused by a real click and given "Al", whose application suggests
 * the destinations starting with it, the first made active by Down; a field
 * holding an accepted suggestion, closed, lower than the sheet's since the
 * kit's rows are taller; and, below the crop, a field whose application is
 * still loading suggestions.
 *
 * The accepted field shows no chevron: the kit's autocomplete opens its list
 * by typing only. Its list hangs a quarter inset below the field. Drawn at
 * the selection sheet's scale through a nested `GuiKit`.
 */
import { useState } from "react";
import type { GuiControlRef } from "@ipp/react/gui";
import {
  Autocomplete,
  GuiKit,
  type GuiKitLayout,
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

/** The f04 crop is 2x of sheet f (776, 522) to (1526, 938); a field added below. */
const EXTENT = [750, 600] as const;

const u = (value: number) => value * K;

/**
 * A pinned cell's margin: the frame glow's reach at the sheet's scale, so the
 * focused field's glow ends inside the cell pasted from its capture.
 */
const GLOW = u(20);

const PLACES: readonly SelectOption[] = [
  { key: "alpha", label: "Alpha Station" },
  { key: "alpine", label: "Alpine Relay" },
  { key: "altair", label: "Altair Dock" },
  { key: "beta", label: "Beta Relay" },
];

/** Each field's top-left, the suggestion rows it shows open and its typing. */
const FIELDS = {
  suggestions: { at: [185, 76], rows: 3, typed: "Al" },
  accepted: { at: [187, 330], rows: 0 },
  loading: { at: [187, 430], rows: 1, typed: "Ga" },
} as const satisfies Record<
  string,
  { at: Point; rows: number; typed?: string }
>;
type Name = keyof typeof FIELDS;

const WIDTH = u(278);
const HEIGHT = u(CONTROL_HEIGHT);

const field = (name: Name): Rect => [...FIELDS[name].at, WIDTH, HEIGHT];

/** The field with its open list: a quarter inset below it, inside its line. */
const open = (name: Name): Rect => [
  ...FIELDS[name].at,
  WIDTH,
  HEIGHT + u(INSET / 4 + 2 * LINE + FIELDS[name].rows * ROW + INSET),
];

/**
 * The application: suggestions are the places whose name starts with the
 * typed text, and one field is always loading more.
 */
function Destination({
  id,
  at,
  defaultText,
  loading,
  control,
}: {
  readonly id: string;
  readonly at: Point;
  readonly defaultText?: string;
  readonly loading?: boolean;
  readonly control: GuiControlRef;
}) {
  const [text, setText] = useState(defaultText ?? "");
  const typed = text.trim().toLowerCase();
  const suggestions =
    typed === ""
      ? []
      : PLACES.filter((place) => place.label.toLowerCase().startsWith(typed));
  const layout: GuiKitLayout = placed(at, WIDTH);
  return (
    <Autocomplete
      id={id}
      label="Destination"
      placeholder="Destination"
      suggestions={suggestions}
      onInputChange={setText}
      {...(defaultText === undefined ? {} : { defaultText })}
      {...(loading ? { loading: "Loading…" } : {})}
      ref={control}
      layout={layout}
    />
  );
}

/** Click the field, which focuses it, and type into it. */
const type = (name: Name, text: string) => [
  { kind: "click" as const, at: centre(field(name)) },
  {
    kind: "action" as const,
    control: name,
    action: { kind: "text" as const, value: text },
  },
  { kind: "settle" as const },
];

const CAPTIONS: Readonly<Record<Name, [string, Rect]>> = {
  suggestions: ["Suggestions", open("suggestions")],
  accepted: ["Accepted suggestion", field("accepted")],
  loading: ["Loading", open("loading")],
};

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "f04-autocomplete.png", origin: [0, 0] },
  states: [
    {
      name: "suggestions",
      cell: around(open("suggestions"), GLOW),
      pin: [
        ...type("suggestions", FIELDS.suggestions.typed),
        { kind: "key", key: "down" },
      ],
    },
    { name: "accepted", cell: around(field("accepted"), 12) },
    {
      name: "loading",
      cell: around(open("loading"), GLOW),
      pin: type("loading", FIELDS.loading.typed),
    },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <GuiKit fontSize={u(TEXT_BODY)}>
        <Destination
          id="auto-suggestions"
          at={FIELDS.suggestions.at}
          control={lab.control("suggestions")}
        />
        <Destination
          id="auto-accepted"
          at={FIELDS.accepted.at}
          defaultText="Alpha Station"
          control={lab.control("accepted")}
        />
      </GuiKit>
      {/* The loading row's spinner stands still, so paint settles. */}
      <GuiKit fontSize={u(TEXT_BODY)} reducedMotion>
        <Destination
          id="auto-loading"
          at={FIELDS.loading.at}
          loading
          control={lab.control("loading")}
        />
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
