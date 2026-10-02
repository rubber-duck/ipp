/**
 * Groups of Button items in the default looks: a segmented group whose
 * selection follows the arrow keys, at rest and after Right moved focus and
 * selection from LOCAL to VIEW; and an option list in the overlay of a
 * focused field, whose rows do not take focus and whose active item the
 * field's arrow keys moved to PULSE while GAIN stays selected. The active row paints as hovered.
 * The sheets show no groups; no reference.
 */
import { Children, Entity } from "@ipp/react";
import { Font, Group, Layout, Overlay, Style, TextInput } from "@ipp/react/gui";
import {
  CentredButton,
  COLUMN,
  Fill,
  Label,
  ROW as ROW_LAYOUT,
  SHEET_CAPTION,
  SHEET_PAGE,
} from "../kit.js";
import { defineSpecimen, type Point, type Rect } from "../specimen.js";
import { CONTROL_HEIGHT, ROW, TEXT_BODY } from "../themes/geometry.js";

const EXTENT = [660, 250] as const;

/** `GuiGroup` values: the axis and the selection the group keeps. */
const HORIZONTAL = 0;
const VERTICAL = 1;
const SELECT_SINGLE = 1;
const SELECT_FOLLOW = 2;

/** `GuiOverlay` values: below the parent's box, stretched along it. */
const OVERLAY_BELOW = 0;
const OVERLAY_STRETCH = 3;

/** A segmented group: three 96-wide segments at the control height. */
const SEGMENTS = ["WORLD", "LOCAL", "VIEW"] as const;
const SEGMENT_WIDTH = 96;
const segmented = (top: number): Rect => [
  15,
  top,
  SEGMENT_WIDTH * SEGMENTS.length,
  CONTROL_HEIGHT,
];
const AT_REST = segmented(28);
const MOVED = segmented(128);

/**
 * The field, and its open overlay 8 below it: the option list's rows at the
 * row height.
 */
const FIELD: Rect = [360, 28, 280, CONTROL_HEIGHT];
const OPTIONS = ["SCAN", "GAIN", "PULSE", "TRACE"] as const;
const SELECTED = "GAIN";
const LIST: Rect = [360, 76, 280, ROW * OPTIONS.length];

/** Rectangles the probes read, by name. */
export const ITEMS = {
  "at-rest": AT_REST,
  moved: MOVED,
  field: FIELD,
  list: LIST,
  ...(Object.fromEntries(
    OPTIONS.map((label, index) => [
      label.toLowerCase(),
      [LIST[0], LIST[1] + ROW * index, LIST[2], ROW] as Rect,
    ]),
  ) as Record<Lowercase<(typeof OPTIONS)[number]>, Rect>),
} as const;

function Segmented({
  id,
  rect,
  font,
  skin,
  control,
}: {
  readonly id: string;
  readonly rect: Rect;
  readonly font: Parameters<typeof CentredButton>[0]["font"];
  readonly skin: Parameters<typeof CentredButton>[0]["skin"];
  readonly control?: Parameters<typeof CentredButton>[0]["control"];
}) {
  const [x, y, width, height] = rect;
  return (
    <Entity id={id}>
      <Layout
        kind={ROW_LAYOUT}
        width={width}
        height={height}
        margin_left={x}
        margin_top={y}
        align_x={-1}
        align_y={-1}
      />
      <Group axis={HORIZONTAL} selection={SELECT_FOLLOW} />
      <Children>
        {SEGMENTS.map((label) => (
          <CentredButton
            key={label}
            id={`${id}/${label.toLowerCase()}`}
            width={SEGMENT_WIDTH}
            height={height}
            label={label}
            font={font}
            size={TEXT_BODY}
            skin={skin}
            selected={label === "LOCAL"}
            {...(label === "LOCAL" && control ? { control } : {})}
          />
        ))}
      </Children>
    </Entity>
  );
}

const up = { kind: "key", key: "up" } as const;
const down = { kind: "key", key: "down" } as const;

export default defineSpecimen({
  extent: EXTENT,
  theme: "items",
  states: [
    { name: "segmented", cell: [6, 10, 306, 90] },
    {
      // Keyboard focus on the selected segment, then Right: focus and
      // selection move to VIEW in one frame. Pressing LOCAL restores it.
      name: "arrow-selects",
      cell: [6, 110, 306, 90],
      pin: [
        { kind: "action", control: "local", action: { kind: "focus" } },
        { kind: "key", key: "right" },
      ],
      restore: [
        { kind: "action", control: "local", action: { kind: "press" } },
      ],
    },
    {
      // Click the field, then Up to the first row and Down twice: from any
      // earlier active row this ends on PULSE, and focus stays on the field.
      name: "option-list",
      cell: [351, 10, 300, 230],
      pin: [
        { kind: "click", at: [FIELD[0] + FIELD[2] - 12, FIELD[1] + 20] },
        up,
        up,
        up,
        down,
        down,
      ],
    },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <Segmented
        id="at-rest"
        rect={AT_REST}
        font={lab.font}
        skin={lab.skin("default")}
      />
      <Segmented
        id="moved"
        rect={MOVED}
        font={lab.font}
        skin={lab.skin("default")}
        control={lab.control("local")}
      />
      <Entity id="field">
        <Layout
          kind={0}
          width={FIELD[2]}
          height={FIELD[3]}
          margin_left={FIELD[0]}
          margin_top={FIELD[1]}
          align_x={-1}
          align_y={-1}
        />
        {lab.skin("default")}
        <Font source={lab.font} font_size={TEXT_BODY} />
        <TextInput text="PU" />
        <Children>
          <Entity id="list">
            <Layout kind={COLUMN} width={LIST[2]} height={LIST[3]} />
            <Overlay side={OVERLAY_BELOW} align={OVERLAY_STRETCH} />
            <Style layer={1} y={LIST[1] - FIELD[1] - FIELD[3]} />
            <Group axis={VERTICAL} selection={SELECT_SINGLE} />
            <Children>
              {OPTIONS.map((label) => (
                <CentredButton
                  key={label}
                  id={`list/${label.toLowerCase()}`}
                  width={LIST[2]}
                  height={ROW}
                  label={label}
                  font={lab.font}
                  size={TEXT_BODY}
                  skin={lab.skin("docked")}
                  selected={label === SELECTED}
                  focusable={false}
                />
              ))}
            </Children>
          </Entity>
        </Children>
      </Entity>
      {[
        { caption: "Segmented, LOCAL selected", at: [15, 82] as Point },
        { caption: "Right moves focus and selection", at: [15, 182] as Point },
        {
          caption: "GAIN selected, PULSE active, field focused",
          at: [360, 232] as Point,
        },
      ].map(({ caption, at }, index) => (
        <Label
          key={caption}
          id={`caption-${index}`}
          at={at}
          text={caption}
          font={lab.font}
          size={12.5}
          color={SHEET_CAPTION}
        />
      ))}
    </>
  ),
});
