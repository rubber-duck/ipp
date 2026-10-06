/**
 * Sheet a, row 05: TextInput idle, focused with its caret, with a selection,
 * and disabled; then hover, which the sheet does not show.
 */
import { Entity } from "@ipp/react";
import { Behavior, Font, Layout, TextInput } from "@ipp/react/gui";
import { Fill, Label, SHEET_CAPTION, SHEET_PAGE } from "../kit.js";
import { defineSpecimen, type Point, type Rect } from "../specimen.js";
import { CONTROL_HEIGHT, TEXT_BODY } from "../themes/geometry.js";

const EXTENT = [960, 100] as const;
const TEXT = "NIGHT-07";
const COLUMNS = [
  { name: "idle", caption: "Idle", x: 16 },
  { name: "focused", caption: "Focused", x: 204.5 },
  { name: "selection", caption: "Selection", x: 391.5 },
  { name: "disabled", caption: "Disabled", x: 579.5 },
  { name: "hover", caption: "Hover", x: 776 },
] as const;

/**
 * The field at the control height, centred where the sheet centres it (y 38).
 * The runtime insets its line one em, the content inset at body type, and
 * centres it vertically.
 */
const input = (x: number): Rect => [
  x,
  38 - CONTROL_HEIGHT / 2,
  168,
  CONTROL_HEIGHT,
];
/** Each column's field rectangle, by column name. */
export const INPUTS = Object.fromEntries(
  COLUMNS.map(({ name, x }) => [name, input(x)]),
) as Readonly<Record<(typeof COLUMNS)[number]["name"], Rect>>;
const cell = (x: number): Rect => [x - 8, 0, 185, 100];
/** A point inside the field to the right of its text: a click there focuses it. */
const inside = ([x, y, width, height]: Rect): Point => [
  x + width - 12,
  y + height / 2,
];
/** A point on the page below the field, away from every control. */
const away = ([x, y, , height]: Rect): Point => [x, y + height + 30];

export default defineSpecimen({
  extent: EXTENT,
  theme: "text-input",
  reference: { image: "a05-text-input.png", origin: [352, 0] },
  states: COLUMNS.map(({ name, x }) => ({
    name,
    cell: cell(x),
    ...(name === "focused"
      ? {
          // UTF-8 offsets: the caret after the last character. The pointer
          // then leaves, so the cell shows focus without hover.
          pin: [
            { kind: "click", at: inside(input(x)) },
            { kind: "selectText", start: TEXT.length, end: TEXT.length },
            { kind: "hover", at: away(input(x)) },
          ] as const,
        }
      : name === "selection"
        ? {
            // "NIGHT" selected, as on the sheet.
            pin: [
              { kind: "click", at: inside(input(x)) },
              { kind: "selectText", start: 0, end: 5 },
              { kind: "hover", at: away(input(x)) },
            ] as const,
          }
        : name === "hover"
          ? { pin: [{ kind: "hover", at: inside(input(x)) }] as const }
          : {}),
  })),
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      {COLUMNS.map(({ name, x }) => {
        const [left, top, width, height] = input(x);
        return (
          <Entity key={name} id={`input-${name}`}>
            <Layout
              kind={0}
              width={width}
              height={height}
              margin_left={left}
              margin_top={top}
              align_x={-1}
              align_y={-1}
            />
            {lab.skin("default")}
            <Font source={lab.font} font_size={TEXT_BODY} />
            {name === "disabled" && <Behavior enabled={false} />}
            <TextInput text={TEXT} ref={lab.control(name)} />
          </Entity>
        );
      })}
      {COLUMNS.map(({ name, caption, x }) => (
        <Label
          key={name}
          id={`caption-${name}`}
          at={[x + 1, 71]}
          text={caption}
          font={lab.font}
          size={12.5}
          color={SHEET_CAPTION}
        />
      ))}
    </>
  ),
});
