/**
 * Sheet d, panel 02: the numeric stepper, drawn with the GUI kit's
 * `NumericStepper`, a numeric TextInput with step parts over -4.00..+4.00
 * stepping by 0.25, idle, focused with its caret after the number and at its
 * upper bound with the increment part disabled, where the sheet has them;
 * below the crop the states the sheet omits: disabled, the increment part
 * hovered, which lights that part alone, and an entry that does not parse,
 * typed and committed with Enter, which keeps the number and shows the
 * kit's error line under the field. The first stepper carries the caption
 * and the range's ends; each has the "EV" unit beside its field.
 *
 * Each field is where the sheet has it; the kit reserves the error line
 * under every field, so a row's reserved line lies under the next row's
 * field and only the last row shows one. Drawn at the value sheet's scale
 * through a nested `GuiKit`.
 */
import { GuiKit, NumericStepper } from "@ipp/react/gui-kit";
import {
  CAPS_HEIGHT,
  CAPS_TOP,
  FONT_ADVANCE,
  Fill,
  Label,
  SHEET_CAPTION,
  SHEET_PAGE,
  placed,
} from "../kit.js";
import { SHEET_D_SCALE as K } from "../scale.js";
import { defineSpecimen, type Point, type Rect } from "../specimen.js";
import {
  CONTROL_HEIGHT,
  DENSE_ROW,
  INSET,
  TEXT_BODY,
  TEXT_SMALL,
} from "../themes/geometry.js";
import { text as textColor } from "../themes/palette.js";

/** The d02 crop, 2x of sheet d (526, 90) to (1010, 490), rows added below. */
const EXTENT = [484, 720] as const;

/** The number's range, step and display, as the sheet states them. */
export const NUMBER = {
  min: -4,
  max: 4,
  step: 0.25,
  fineStep: 0.05,
  precision: 2,
} as const;

/**
 * Each row's field middle, measured on the sheet where it has the row, and
 * its caption; the added rows continue at the sheet's pitch.
 */
const ROWS = [
  { name: "idle", middle: 154.5, caption: "Idle", value: 1.25 },
  { name: "focus", middle: 242.5, caption: "Focus", value: 1.25 },
  { name: "limit", middle: 331, caption: "At limit", value: 4 },
  { name: "disabled", middle: 419.5, caption: "Disabled", value: 1.25 },
  { name: "hover", middle: 508, caption: "Hover +", value: 1.25 },
  { name: "invalid", middle: 596.5, caption: "Invalid", value: 1.25 },
] as const;
type Row = (typeof ROWS)[number]["name"];

const u = (value: number) => value * K;
const BODY = u(TEXT_BODY);
const SMALL = u(TEXT_SMALL);

/** The field: the sheet's 278 wide at the control height, from x 103.5. */
const FIELD = { x: 103.5, width: 278, height: u(CONTROL_HEIGHT) };

/** The stepper: the field, half an inset, and the unit as the kit fits it. */
const STEPPER_WIDTH =
  FIELD.width + u(INSET / 2) + 2 * FONT_ADVANCE * BODY + BODY / 100;

const field = (middle: number): Rect => [
  FIELD.x,
  middle - FIELD.height / 2,
  FIELD.width,
  FIELD.height,
];

/** Each row's field rectangle, by row name. */
export const FIELDS = Object.fromEntries(
  ROWS.map(({ name, middle }) => [name, field(middle)]),
) as Readonly<Record<Row, Rect>>;

/** The centre of the increment part, the square of the field's height. */
const increment = ([x, y, width, height]: Rect): Point => [
  x + width - height / 2,
  y + height / 2,
];

/** A point in the text area right of the centred number: a click there focuses it. */
const inside = ([x, y, width, height]: Rect): Point => [
  x + width / 2 + (2 * height) / 3,
  y + height / 2,
];

/** A point on the page left of the field, away from every control. */
const away = ([x, y, , height]: Rect): Point => [x - 40, y + height / 2];

/** Each row reaches halfway to its neighbours, so glows stay in their cells. */
const cell = (index: number): Rect => {
  const middle = ROWS[index]!.middle;
  const top = index ? (ROWS[index - 1]!.middle + middle) / 2 : 90;
  const next = ROWS[index + 1];
  const bottom = next ? (middle + next.middle) / 2 : EXTENT[1];
  return [0, top, EXTENT[0], bottom - top];
};

const CAPTION_SIZE = 12.5 * K;
const NOTE = "Step 0.25 / clamp to limits.";

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "d02-numeric-stepper.png", origin: [0, 0] },
  states: ROWS.map(({ name }, index) => {
    const rect = FIELDS[name];
    switch (name) {
      case "focus":
        // The caret after "1.25", then the pointer leaves, so the cell shows
        // focus without hover.
        return {
          name,
          cell: cell(index),
          pin: [
            { kind: "click", at: inside(rect) },
            { kind: "selectText", start: 4, end: 4 },
            { kind: "hover", at: away(rect) },
          ] as const,
        };
      case "hover":
        return {
          name,
          cell: cell(index),
          pin: [{ kind: "hover", at: increment(rect) }] as const,
        };
      case "invalid":
        // "abc" replaces "1.25" and Enter refuses it: the edit stays for
        // correction, the number stays, and the kit shows its error.
        return {
          name,
          cell: cell(index),
          pin: [
            { kind: "click", at: inside(rect) },
            { kind: "selectText", start: 0, end: 4 },
            { kind: "typeText", text: "abc" },
            { kind: "key", key: "enter" },
            { kind: "settle" },
            { kind: "hover", at: away(rect) },
          ] as const,
        };
      default:
        return { name, cell: cell(index) };
    }
  }),
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <Label
        id="note"
        at={[464 - NOTE.length * FONT_ADVANCE * SMALL, 78.5 - CAPS_TOP * SMALL]}
        width={NOTE.length * FONT_ADVANCE * SMALL + 4}
        text={NOTE}
        font={lab.font}
        size={SMALL}
        color={textColor}
      />
      <GuiKit fontSize={BODY}>
        {ROWS.map(({ name, value }, index) => {
          const [x, y] = FIELDS[name];
          const first = index === 0;
          return (
            <NumericStepper
              key={name}
              id={`stepper-${name}`}
              {...(first ? { label: "EXPOSURE" } : { bounds: false })}
              {...NUMBER}
              units="EV"
              defaultValue={value}
              disabled={name === "disabled"}
              ref={lab.control(name)}
              layout={placed([x, first ? y - u(DENSE_ROW) : y], STEPPER_WIDTH)}
            />
          );
        })}
      </GuiKit>
      {ROWS.map(({ name, middle, caption }) => (
        <Label
          key={name}
          id={`caption-${name}`}
          at={[20, middle - (CAPS_TOP + CAPS_HEIGHT / 2) * CAPTION_SIZE]}
          text={caption}
          font={lab.font}
          size={CAPTION_SIZE}
          color={SHEET_CAPTION}
        />
      ))}
    </>
  ),
});
