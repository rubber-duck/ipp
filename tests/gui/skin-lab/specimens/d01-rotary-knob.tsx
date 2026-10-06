/**
 * Sheet d, panel 01: the rotary knob, drawn with the GUI kit's `Knob`, a
 * Slider presented as a dial in its housing, at 65% idle, focused and
 * disabled where the sheet has them, and to the right of the crop the states
 * the sheet omits: hover, dragged up to 80% by a real held drag, and a
 * bipolar knob over -100..+100 at -30 that fills from zero. The kit puts each
 * knob's caption above its housing, the captions of the range's ends under
 * the sweep's ends and the readout under the dial, inside the housing.
 *
 * The kit's housing is the language's 80-unit dial, smaller than the sheet's
 * 100; each knob is centred where the sheet centres its housing. A second row
 * shows the knob with its paired numeric input, a `NumericStepper` without
 * step parts sharing the knob's value, idle and after "80" is typed into it
 * and committed with Enter, which turns the knob; and a bipolar knob at
 * `size` 112, whose longer end captions have room. Drawn at the value sheet's
 * scale through a nested `GuiKit`.
 */
import { useState } from "react";
import { GuiKit, Knob, NumericStepper } from "@ipp/react/gui-kit";
import type { GuiControlRef } from "@ipp/react/gui";
import {
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
  DIAL,
  INSET,
  TEXT_BODY,
  TEXT_SMALL,
} from "../themes/geometry.js";
import { text as textColor } from "../themes/palette.js";

/**
 * The d01 crop, 2x of sheet d, with three columns added to its right and a
 * row below.
 */
const EXTENT = [1000, 700] as const;

/** The first row's cells end above the second row. */
const FIRST_ROW = 330;

/** The factor every design-language length is drawn at. */
export const SCALE = K;

/** The value and the dragged value as fractions of the 0..100% range. */
export const VALUE = 0.65;
export const DRAGGED = 0.8;

/** The bipolar knob's range, value and fill origin. */
export const BIPOLAR = { min: -100, max: 100, value: -30, origin: 0 } as const;

/** The kit's housing: the dial's side wide and a dense row taller. */
const SIDE = DIAL * K;
const HOUSING = [SIDE, SIDE + DENSE_ROW * K] as const;
/** The housing's top, the sheet's; the caption's row sits above it. */
const TOP = 129.5;

/**
 * Each column's centre: the sheet's three housings, then three more at its
 * pitch.
 */
const COLUMNS = [
  { name: "idle", caption: "Idle", x: 91 },
  { name: "focus", caption: "Focus", x: 252.5 },
  { name: "disabled", caption: "Disabled", x: 413 },
  { name: "hover", caption: "Hover", x: 574 },
  { name: "dragging", caption: "Dragging", x: 735 },
  { name: "bipolar", caption: "Bipolar", x: 896 },
] as const;
type Column = (typeof COLUMNS)[number]["name"];

/** Each knob's housing, the control's rectangle, by column name. */
export const DIALS = Object.fromEntries(
  COLUMNS.map(({ name, x }) => [
    name,
    [x - HOUSING[0] / 2, TOP, ...HOUSING] as Rect,
  ]),
) as Readonly<Record<Column, Rect>>;

/** The dial's centre: the middle of the housing's top square. */
export const dialCentre = ([x, y, width]: Rect): Point => [
  x + width / 2,
  y + width / 2,
];

/**
 * Upward travel that crosses a dial's whole range: two and a half times its
 * side, the runtime's documented drag rule.
 */
const travel = ([, , width]: Rect) => 2.5 * width;

/** Each column reaches halfway to its neighbours, so glows stay in their cells. */
const CELLS: readonly Rect[] = COLUMNS.map(({ x }, index) => {
  const left = index ? (COLUMNS[index - 1]!.x + x) / 2 : 0;
  const next = COLUMNS[index + 1];
  const right = next ? (x + next.x) / 2 : EXTENT[0];
  return [left, 0, right - left, FIRST_ROW];
});

/**
 * The second row: the paired knobs, as wide as the paired input's error
 * message needs, and the larger bipolar knob, each centred on `x`.
 */
const PAIRED_WIDTH = 120 * K;
const LARGER = 112;
const SECOND = [
  { name: "paired", caption: "Paired input", x: 120 },
  { name: "typed", caption: "Typed 80", x: 340 },
  { name: "larger", caption: "Size 112", x: 580 },
] as const;
const SECOND_TOP = 345;

/** The second row's knob roots, by name. */
const SECOND_ROOTS = Object.fromEntries(
  SECOND.map(({ name, x }) => {
    const width = name === "larger" ? LARGER * K : PAIRED_WIDTH;
    return [name, [x - width / 2, SECOND_TOP, width] as const];
  }),
) as Readonly<
  Record<(typeof SECOND)[number]["name"], readonly [number, number, number]>
>;

/**
 * The paired input's field under a paired knob: below the caption, the
 * housing and half an inset, as wide as the knob less the unit beside it.
 */
function pairedField(name: "paired" | "typed"): Rect {
  const [x, y, width] = SECOND_ROOTS[name];
  const top = y + (DENSE_ROW + DIAL + DENSE_ROW + INSET / 2) * K;
  const unit = FONT_ADVANCE * TEXT_BODY * K + (TEXT_BODY * K) / 100;
  return [x, top, width - (INSET / 2) * K - unit, CONTROL_HEIGHT * K];
}

/** A knob and its paired numeric input, both controlled by one value. */
function PairedKnob({
  id,
  layout,
  control,
}: {
  readonly id: string;
  readonly layout: ReturnType<typeof placed>;
  readonly control: GuiControlRef;
}) {
  const [gain, setGain] = useState(Math.round(VALUE * 100));
  return (
    <Knob
      id={id}
      label="GAIN"
      min={0}
      max={100}
      step={1}
      units="%"
      value={gain}
      onChange={setGain}
      ref={control}
      layout={layout}
    >
      <NumericStepper
        id={`${id}/input`}
        min={0}
        max={100}
        units="%"
        stepParts={false}
        bounds={false}
        value={gain}
        onChange={setGain}
      />
    </Knob>
  );
}

const SMALL = TEXT_SMALL * K;
const CAPTION_SIZE = 12.5 * K;
const HINT = "Drag adjusts / type for precision.";

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "d01-rotary-knob.png", origin: [0, 0] },
  states: [
    ...COLUMNS.map(({ name }, index) => {
      const cell = CELLS[index]!;
      const centre = dialCentre(DIALS[name]);
      switch (name) {
        case "hover":
          return { name, cell, pin: [{ kind: "hover", at: centre }] as const };
        case "focus":
          return {
            name,
            cell,
            pin: [
              { kind: "action", control: name, action: { kind: "focus" } },
            ] as const,
          };
        case "dragging": {
          // A real held drag up from the dial's centre: the press keeps 65%
          // and the travel adds 15% of the range.
          const rise = (DRAGGED - VALUE) * travel(DIALS[name]);
          return {
            name,
            cell,
            pin: [
              {
                kind: "drag",
                from: centre,
                to: [centre[0], centre[1] - rise],
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
        }
        default:
          return { name, cell };
      }
    }),
    ...SECOND.map(({ name, x }) => {
      const cell: Rect = [x - 110, FIRST_ROW, 220, EXTENT[1] - FIRST_ROW];
      if (name !== "typed") return { name, cell };
      // "80" replaces "65" in the paired input and Enter commits it; the
      // application's value turns the knob a client round trip later.
      const [fx, fy, fw, fh] = pairedField(name);
      return {
        name,
        cell,
        pin: [
          { kind: "click", at: [fx + fw * 0.8, fy + fh / 2] },
          { kind: "selectText", start: 0, end: 2 },
          { kind: "typeText", text: "80" },
          { kind: "key", key: "enter" },
          { kind: "settle" },
          { kind: "settle" },
          { kind: "hover", at: [x, EXTENT[1] - 10] },
        ] as const,
        restore: [
          {
            kind: "action",
            control: name,
            action: { kind: "scalar", value: Math.round(VALUE * 100) },
          },
        ] as const,
      };
    }),
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <Label
        id="hint"
        at={[482 - HINT.length * FONT_ADVANCE * SMALL, 77.5 - CAPS_TOP * SMALL]}
        width={HINT.length * FONT_ADVANCE * SMALL + 4}
        text={HINT}
        font={lab.font}
        size={SMALL}
        color={textColor}
      />
      <GuiKit fontSize={TEXT_BODY * K}>
        {COLUMNS.map(({ name }) => {
          const [x, y] = DIALS[name];
          const at: Point = [x, y - DENSE_ROW * K];
          return name === "bipolar" ? (
            <Knob
              key={name}
              id={`knob-${name}`}
              label="PAN"
              min={BIPOLAR.min}
              max={BIPOLAR.max}
              step={1}
              fineStep={0.1}
              origin={BIPOLAR.origin}
              defaultValue={BIPOLAR.value}
              ref={lab.control(name)}
              layout={placed(at)}
            />
          ) : (
            <Knob
              key={name}
              id={`knob-${name}`}
              label="GAIN"
              min={0}
              max={100}
              step={1}
              fineStep={0.1}
              units="%"
              defaultValue={Math.round(VALUE * 100)}
              disabled={name === "disabled"}
              ref={lab.control(name)}
              layout={placed(at)}
            />
          );
        })}
        {(["paired", "typed"] as const).map((name) => {
          const [x, y, width] = SECOND_ROOTS[name];
          return (
            <PairedKnob
              key={name}
              id={`knob-${name}`}
              control={lab.control(name)}
              layout={placed([x, y], width)}
            />
          );
        })}
        <Knob
          id="knob-larger"
          label="PAN"
          min={BIPOLAR.min}
          max={BIPOLAR.max}
          step={1}
          origin={BIPOLAR.origin}
          defaultValue={BIPOLAR.value}
          size={LARGER}
          layout={placed([SECOND_ROOTS.larger[0], SECOND_ROOTS.larger[1]])}
        />
      </GuiKit>
      {[
        ...COLUMNS.map((column) => ({ ...column, y: 300 })),
        ...SECOND.map((column) => ({ ...column, y: 675 })),
      ].map(({ name, caption, x, y }) => (
        <Label
          key={name}
          id={`caption-${name}`}
          at={[x - 60, y - CAPS_TOP * CAPTION_SIZE]}
          width={120}
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
