/**
 * Sheet d, the range slider, drawn with the GUI kit's `RangeSlider`: a
 * distance over 0 to 100 m held between thumbs at 20 and 80, idle, with
 * keyboard focus on the lower thumb and disabled; then, below the crop,
 * states the sheet omits: the upper thumb hovered and the lower thumb
 * pressed, which light only that thumb. Each thumb is a focus part of one
 * slider, so the focus pin names its part. The kit puts the range's ends
 * beside the rail and each readout under its thumb; the first row carries
 * the caption.
 *
 * Each rail runs where the sheet's does, from 111 to 426. Drawn at the value
 * sheet's scale through a nested `GuiKit`.
 */
import { Fragment } from "react";
import { GuiKit, RangeSlider } from "@ipp/react/gui-kit";
import {
  CAPS_TOP,
  FONT_ADVANCE,
  FONT_LINE,
  Fill,
  Label,
  SHEET_CAPTION,
  SHEET_PAGE,
  placed,
} from "../kit.js";
import { SHEET_D_SCALE as K } from "../scale.js";
import { defineSpecimen, type Point, type Rect } from "../specimen.js";
import { DENSE_ROW, INSET, TEXT_BODY, TEXT_SMALL } from "../themes/geometry.js";

/** The d05 crop is 2x of sheet d (13, 501) to (515, 937); rows added below. */
const EXTENT = [502, 600] as const;

export const RANGE = { min: 0, max: 100 } as const;
export const VALUES = [20, 80] as const;

/**
 * Each row's rail middle and caption lines, measured on the sheet where it
 * has the row.
 */
const ROWS = [
  { name: "idle", middle: 153.5, caption: ["Idle"] },
  { name: "focus", middle: 246.5, caption: ["Focus", "(lower)"] },
  { name: "disabled", middle: 345.5, caption: ["Disabled"] },
  { name: "hover", middle: 440, caption: ["Hover", "(upper)"] },
  { name: "dragging", middle: 530, caption: ["Dragging", "(lower)"] },
] as const;
type Row = (typeof ROWS)[number]["name"];
const RAIL = { start: 111, end: 426 };
const CAPTION_X = 20;

const u = (value: number) => value * K;
/** The kit's slider depth across its rail and its thumb, a quarter less. */
const DEPTH = u((TEXT_BODY * 4) / 3);
const THUMB = 0.75 * DEPTH;
const SMALL = u(TEXT_SMALL);
/** A caption's width beside the rail, as the kit fits it. */
const width = (text: string) =>
  [...text].length * FONT_ADVANCE * SMALL + SMALL / 100;
const GAP = u(INSET / 2);

/** Each row's slider rectangle, by row name. */
export const SLIDERS = Object.fromEntries(
  ROWS.map(({ name, middle }) => [
    name,
    [RAIL.start, middle - DEPTH / 2, RAIL.end - RAIL.start, DEPTH] as Rect,
  ]),
) as Readonly<Record<Row, Rect>>;

/** A thumb's centre at `value`, by the runtime's mapping. */
function thumb([x, y, length, depth]: Rect, value: number): Point {
  const fraction = (value - RANGE.min) / (RANGE.max - RANGE.min);
  return [x + THUMB / 2 + fraction * (length - THUMB), y + depth / 2];
}

/** Each row reaches halfway to its neighbours, so glows stay in their cells. */
const cell = (index: number): Rect => {
  const middle = ROWS[index]!.middle;
  const top = index ? (ROWS[index - 1]!.middle + middle) / 2 : 100;
  const next = ROWS[index + 1];
  const bottom = next ? (middle + next.middle) / 2 : EXTENT[1];
  return [0, top, EXTENT[0], bottom - top];
};

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "d05-range-slider.png", origin: [0, 0] },
  states: ROWS.map(({ name }, index) => {
    const rect = SLIDERS[name];
    switch (name) {
      case "focus":
        return {
          name,
          cell: cell(index),
          pin: [
            {
              kind: "action",
              control: name,
              action: { kind: "focus", part: 0 },
            },
          ] as const,
        };
      case "hover":
        return {
          name,
          cell: cell(index),
          pin: [{ kind: "hover", at: thumb(rect, VALUES[1]) }] as const,
        };
      case "dragging":
        return {
          name,
          cell: cell(index),
          pin: [{ kind: "press", at: thumb(rect, VALUES[0]) }] as const,
        };
      default:
        return { name, cell: cell(index) };
    }
  }),
  render: (lab) => {
    const left = RAIL.start - GAP - width("0 m");
    const span = RAIL.end + GAP + width("100 m") - left;
    return (
      <>
        <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
        <GuiKit fontSize={u(TEXT_BODY)}>
          {ROWS.map(({ name, middle }) => (
            <RangeSlider
              key={name}
              id={`range-${name}`}
              {...(name === "idle" ? { label: "DISTANCE" } : {})}
              min={RANGE.min}
              max={RANGE.max}
              step={1}
              fineStep={0.1}
              units="m"
              defaultValue={VALUES}
              disabled={name === "disabled"}
              ref={lab.control(name)}
              layout={placed(
                [
                  left,
                  middle - DEPTH / 2 - (name === "idle" ? u(DENSE_ROW) : 0),
                ],
                span,
              )}
            />
          ))}
        </GuiKit>
        {ROWS.map(({ name, middle, caption }) => (
          <Fragment key={name}>
            {caption.map((line, index) => (
              <Label
                key={line}
                id={`caption-${name}-${index}`}
                at={[
                  CAPTION_X,
                  middle +
                    THUMB / 2 +
                    GAP / 2 +
                    index * FONT_LINE * SMALL -
                    CAPS_TOP * SMALL,
                ]}
                text={line}
                font={lab.font}
                size={SMALL}
                color={SHEET_CAPTION}
              />
            ))}
          </Fragment>
        ))}
      </>
    );
  },
});
