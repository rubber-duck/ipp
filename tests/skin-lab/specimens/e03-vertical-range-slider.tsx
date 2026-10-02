/**
 * Sheet e, panel 03: two vertical range sliders over 0 to 100 m holding 20
 * to 80, the second with keyboard focus on its upper thumb, drawn with the
 * GUI kit's `RangeSlider`. The lower thumb stays below the upper one and the
 * fill runs only between them. Each thumb is a focus part of one slider, so
 * the focus pin names the upper part. The kit puts each readout right of its
 * thumb, where the sheet labels its scale, the range's ends above and below
 * the rail and the caption above them; with readouts that follow the thumbs
 * the range needs no scale.
 *
 * Drawn at sheet e's scale through a nested `GuiKit`; each rail runs where
 * the sheet's does.
 */
import { GuiKit, RangeSlider } from "@ipp/react/gui-kit";
import {
  CAPS_TOP,
  Fill,
  Label,
  SHEET_CAPTION,
  SHEET_PAGE,
  placed,
} from "../kit.js";
import { SHEET_E_SCALE as K } from "../scale.js";
import { defineSpecimen, type Rect } from "../specimen.js";
import { DENSE_ROW, TEXT_BODY } from "../themes/geometry.js";

/** The e03 crop, sheet e (751, 518) to (1524, 961). */
const EXTENT = [773, 443] as const;

export const RANGE = { min: 0, max: 100 } as const;
export const VALUES = [20, 80] as const;

/** The factor every design-language length is drawn at. */
export const SCALE = K;

/**
 * Each slider's rail centre and state caption; the thumb centre travels from
 * the sheet's 0 m tick up to its 100 m tick.
 */
const COLUMNS = [
  { name: "idle", caption: "Idle", x: 176.5 },
  { name: "focus", caption: "Focus upper", x: 550.5 },
] as const;
const TRAVEL = { top: 114, bottom: 352 };
const CAPTION_SIZE = 12.5 * K;

/** The kit slider's thumb, its depth's three quarters: one em of its body. */
const THUMB = TEXT_BODY * K;
/** The rail's length: the travel and a thumb, in the World's units. */
const RAIL = TRAVEL.bottom - TRAVEL.top + THUMB;
const ROW = DENSE_ROW * K;
const COLUMN_WIDTH = 220;

/** Each slider's rectangle, by state name. */
export const SLIDERS = Object.fromEntries(
  COLUMNS.map(({ name, x }) => [
    name,
    [
      x - (THUMB * 2) / 3,
      TRAVEL.top - THUMB / 2,
      (THUMB * 4) / 3,
      RAIL,
    ] as Rect,
  ]),
) as Readonly<Record<(typeof COLUMNS)[number]["name"], Rect>>;

/** Each column's cell: its caption, ends, rail and state caption, with glow. */
const cell = (x: number): Rect => [x - 110, 20, 220, 410];

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "e03-vertical-range-slider.png", origin: [0, 0] },
  states: COLUMNS.map(({ name, x }) =>
    name === "focus"
      ? {
          name,
          cell: cell(x),
          pin: [
            {
              kind: "action",
              control: name,
              action: { kind: "focus", part: 1 },
            },
          ] as const,
        }
      : { name, cell: cell(x) },
  ),
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <GuiKit fontSize={TEXT_BODY * K}>
        {COLUMNS.map(({ name, x }) => (
          <RangeSlider
            key={name}
            id={`range-${name}`}
            label="DISTANCE"
            min={RANGE.min}
            max={RANGE.max}
            step={1}
            fineStep={0.1}
            units="m"
            defaultValue={VALUES}
            vertical
            length={RAIL / K}
            ref={lab.control(name)}
            layout={placed(
              [x - COLUMN_WIDTH / 2, SLIDERS[name][1] - 2 * ROW],
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
            x - 80,
            SLIDERS[name][1] + RAIL + ROW + 12 - CAPS_TOP * CAPTION_SIZE,
          ]}
          width={160}
          centred
          text={caption}
          font={lab.font}
          size={CAPTION_SIZE}
          color={SHEET_CAPTION}
        />
      ))}
    </>
  ),
});
