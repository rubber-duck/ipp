/**
 * Sheet e, panel 01: the vertical slider at 65% idle, hover, focus, dragged
 * to 80% and disabled, drawn with the GUI kit's `LabelledSlider` and its
 * scale. Sliders run along their vertical axis with the minimum at the
 * bottom; the kit centres each rail in its column, puts the scale right of
 * it and the readout under it.
 *
 * The sheet's row is 1512 units wide, more than a canvas captures, so the
 * specimen lays it out at two thirds of the sheet's size and reads the 2x
 * reference crop at three crop pixels per unit: placements are the sheet's
 * times `FIT`, and every length is a design-language value times `K`, sheet
 * e's scale times `FIT`, through a nested `GuiKit`.
 */
import { GuiKit, LabelledSlider } from "@ipp/react/gui-kit";
import {
  CAPS_TOP,
  Fill,
  Label,
  SHEET_CAPTION,
  SHEET_PAGE,
  placed,
} from "../kit.js";
import { SHEET_E_SCALE } from "../scale.js";
import { defineSpecimen, type Point, type Rect } from "../specimen.js";
import { DENSE_ROW, TEXT_BODY } from "../themes/geometry.js";

const FIT = 2 / 3;
const K = SHEET_E_SCALE * FIT;

/** The factor every design-language length is drawn at. */
export const SCALE = K;

/** The e01 crop, sheet e (12, 110) to (1524, 508), at `FIT`. */
const EXTENT = [1008, 265] as const;

/** The value and the dragged value as fractions of the 0..100% range. */
export const VALUE = 0.65;
export const DRAGGED = 0.8;

/**
 * Each column's rail centre and the thumb centre's travel from the sheet's
 * 100 tick to its 0 tick, in crop units before `FIT`.
 */
const COLUMNS = [
  { name: "idle", caption: "Idle", x: 154 },
  { name: "hover", caption: "Hover", x: 450 },
  { name: "focus", caption: "Focus", x: 743 },
  { name: "dragging", caption: "Dragging", x: 1025 },
  { name: "disabled", caption: "Disabled", x: 1316 },
] as const;
const TRAVEL = { top: 77.9, bottom: 313.7 };
const CAPTION_CAPS = 363;
const CAPTION_SIZE = 12.5;

/** The kit slider's thumb, its depth's three quarters: one em of its body. */
const THUMB = TEXT_BODY * K;
/** The rail's length: the travel and a thumb, in the World's units. */
const RAIL = (TRAVEL.bottom - TRAVEL.top) * FIT + THUMB;
const COLUMN_WIDTH = 180;

/** Each column's slider rectangle, by column name. */
export const SLIDERS = Object.fromEntries(
  COLUMNS.map(({ name, x }) => [
    name,
    [
      x * FIT - (THUMB * 2) / 3,
      TRAVEL.top * FIT - THUMB / 2,
      (THUMB * 4) / 3,
      RAIL,
    ] as Rect,
  ]),
) as Readonly<Record<(typeof COLUMNS)[number]["name"], Rect>>;

/** The thumb's centre at a fraction of the range, by the runtime's mapping. */
function thumb([x, y, width, height]: Rect, fraction: number): Point {
  return [x + width / 2, y + height - THUMB / 2 - fraction * (height - THUMB)];
}

/** Each column reaches halfway to its neighbours, so glows stay in their cells. */
const CELLS: readonly Rect[] = COLUMNS.map(({ x }, index) => {
  const left = index ? ((COLUMNS[index - 1]!.x + x) / 2) * FIT : 0;
  const next = COLUMNS[index + 1];
  const right = next ? ((x + next.x) / 2) * FIT : EXTENT[0];
  return [left, 0, right - left, EXTENT[1]];
});

export default defineSpecimen({
  extent: EXTENT,
  reference: {
    image: "e01-vertical-slider.png",
    origin: [0, 0],
    scale: 2 / FIT,
  },
  states: COLUMNS.map(({ name }, index) => {
    const rect = SLIDERS[name];
    const cell = CELLS[index]!;
    switch (name) {
      case "hover":
        return {
          name,
          cell,
          pin: [{ kind: "hover", at: thumb(rect, VALUE) }] as const,
        };
      case "focus":
        return {
          name,
          cell,
          pin: [
            { kind: "action", control: name, action: { kind: "focus" } },
          ] as const,
        };
      case "dragging":
        // A real held drag from the thumb at 65% up to 80%.
        return {
          name,
          cell,
          pin: [
            {
              kind: "drag",
              from: thumb(rect, VALUE),
              to: thumb(rect, DRAGGED),
            },
          ] as const,
          restore: [
            {
              kind: "action",
              control: name,
              action: { kind: "scalar", value: Math.round(VALUE * 100) },
            },
          ] as const,
        };
      default:
        return { name, cell };
    }
  }),
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <GuiKit fontSize={TEXT_BODY * K}>
        {COLUMNS.map(({ name, x }) => (
          <LabelledSlider
            key={name}
            id={`slider-${name}`}
            min={0}
            max={100}
            step={1}
            fineStep={0.1}
            units="%"
            defaultValue={Math.round(VALUE * 100)}
            vertical
            length={RAIL / K}
            scale={{ count: 5 }}
            disabled={name === "disabled"}
            ref={lab.control(name)}
            layout={placed(
              [x * FIT - COLUMN_WIDTH / 2, SLIDERS[name][1]],
              COLUMN_WIDTH,
            )}
          />
        ))}
      </GuiKit>
      {COLUMNS.map(({ name, caption, x }) => (
        <Label
          key={name}
          id={`caption-${name}`}
          at={[
            x * FIT - 60,
            Math.max(
              CAPTION_CAPS * FIT,
              SLIDERS[name][1] + RAIL + DENSE_ROW * K + 4,
            ) -
              CAPS_TOP * CAPTION_SIZE * K,
          ]}
          width={120}
          centred
          text={caption}
          font={lab.font}
          size={CAPTION_SIZE * K}
          color={SHEET_CAPTION}
        />
      ))}
    </>
  ),
});
