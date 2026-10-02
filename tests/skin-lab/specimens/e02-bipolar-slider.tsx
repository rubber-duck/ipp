/**
 * Sheet e, panel 02: two bipolar vertical sliders over -100..+100 with their
 * fill origin at zero, PAN at -30 and OFFSET at +40, drawn with the GUI
 * kit's `LabelledSlider`. The fill runs only between zero and the thumb, down
 * for a negative value and up for a positive one. The kit's scale marks zero,
 * the slider's origin, from the rail, the explicit centre reference the sheet
 * asks for, and signs the positive side, as the readout does; the caption
 * names the value above the rail and the readout carries it below.
 *
 * Drawn at sheet e's scale through a nested `GuiKit`; each rail runs where
 * the sheet's does.
 */
import { GuiKit, LabelledSlider } from "@ipp/react/gui-kit";
import { Fill, SHEET_PAGE, placed } from "../kit.js";
import { SHEET_E_SCALE as K } from "../scale.js";
import { defineSpecimen, type Rect } from "../specimen.js";
import { DENSE_ROW, TEXT_BODY } from "../themes/geometry.js";

/** The e02 crop, sheet e (12, 518) to (738, 961). */
const EXTENT = [726, 443] as const;

export const RANGE = { min: -100, max: 100 } as const;

/** The factor every design-language length is drawn at. */
export const SCALE = K;

/**
 * Each slider's rail centre, value and caption; the thumb centre travels
 * from the sheet's -100 tick to its +100 tick.
 */
const SLIDER_COLUMNS = [
  {
    name: "negative",
    label: "PAN",
    value: -30,
    x: 163.25,
    cell: [60, 50, 250, 340],
  },
  {
    name: "positive",
    label: "OFFSET",
    value: 40,
    x: 505.5,
    cell: [400, 50, 280, 340],
  },
] as const;
const TRAVEL = { top: 115.5, bottom: 335 };

/** The kit slider's thumb, its depth's three quarters: one em of its body. */
const THUMB = TEXT_BODY * K;
/** The rail's length: the travel and a thumb, in the World's units. */
const RAIL = TRAVEL.bottom - TRAVEL.top + THUMB;
const COLUMN_WIDTH = 240;

/** Each slider's rectangle and value, by state name. */
export const BIPOLAR = Object.fromEntries(
  SLIDER_COLUMNS.map(({ name, x, value }) => [
    name,
    {
      rect: [
        x - (THUMB * 2) / 3,
        TRAVEL.top - THUMB / 2,
        (THUMB * 4) / 3,
        RAIL,
      ] as Rect,
      value,
    },
  ]),
) as Readonly<
  Record<
    (typeof SLIDER_COLUMNS)[number]["name"],
    { readonly rect: Rect; readonly value: number }
  >
>;

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "e02-bipolar-slider.png", origin: [0, 0] },
  states: SLIDER_COLUMNS.map(({ name, cell }) => ({ name, cell })),
  render: () => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <GuiKit fontSize={TEXT_BODY * K}>
        {SLIDER_COLUMNS.map(({ name, label, value, x }) => (
          <LabelledSlider
            key={name}
            id={`slider-${name}`}
            label={label}
            min={RANGE.min}
            max={RANGE.max}
            step={1}
            fineStep={0.1}
            origin={0}
            units="%"
            defaultValue={value}
            vertical
            length={RAIL / K}
            scale={{ count: 5 }}
            layout={placed(
              [x - COLUMN_WIDTH / 2, BIPOLAR[name].rect[1] - DENSE_ROW * K],
              COLUMN_WIDTH,
            )}
          />
        ))}
      </GuiKit>
    </>
  ),
});
