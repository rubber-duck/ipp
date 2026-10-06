/** Sheet a, row 01: Button in idle, hover, pressed, focus, disabled and the amber variant. */
import {
  CentredButton,
  Fill,
  Label,
  SHEET_CAPTION,
  SHEET_PAGE,
  centre,
} from "../kit.js";
import { defineSpecimen, type Rect } from "../specimen.js";
import { CONTROL_HEIGHT, TEXT_BODY } from "../themes/geometry.js";

const EXTENT = [770, 107] as const;

/**
 * Every button is 108 wide at the control height, amber included, centred
 * where the sheet centres its buttons (y 44); `x` is each button's left edge
 * and `caption_x` the left edge of its caption.
 */
const COLUMNS = [
  { name: "idle", caption: "Idle", x: 15, caption_x: 17 },
  { name: "hover", caption: "Hover", x: 141.75, caption_x: 145 },
  { name: "pressed", caption: "Pressed", x: 267.5, caption_x: 271 },
  { name: "focus", caption: "Focus", x: 393, caption_x: 397 },
  { name: "disabled", caption: "Disabled", x: 520.25, caption_x: 525 },
  { name: "amber", caption: "Variant (amber)", x: 639, caption_x: 651 },
] as const;

const WIDTH = 108;

const button = (index: number): Rect => [
  COLUMNS[index]!.x,
  44 - CONTROL_HEIGHT / 2,
  WIDTH,
  CONTROL_HEIGHT,
];

/** Each column's button rectangle, by column name. */
export const BUTTONS = Object.fromEntries(
  COLUMNS.map(({ name }, index) => [name, button(index)]),
) as Readonly<Record<(typeof COLUMNS)[number]["name"], Rect>>;

/** Cells reach halfway to the neighbouring buttons, so each glow stays in its cell. */
const cell = (index: number): Rect => {
  const edge = (between: number) =>
    between < 0
      ? 0
      : between >= COLUMNS.length - 1
        ? EXTENT[0]
        : (button(between)[0] + button(between)[2] + button(between + 1)[0]) /
          2;
  const left = edge(index - 1);
  return [left, 0, edge(index) - left, EXTENT[1]];
};

export default defineSpecimen({
  extent: EXTENT,
  theme: "button",
  reference: { image: "a01-button.png", origin: [352, 0] },
  states: [
    { name: "idle", cell: cell(0) },
    {
      name: "hover",
      cell: cell(1),
      pin: [{ kind: "hover", at: centre(button(1)) }],
    },
    {
      name: "pressed",
      cell: cell(2),
      pin: [{ kind: "press", at: centre(button(2)) }],
    },
    {
      name: "focus",
      cell: cell(3),
      pin: [{ kind: "action", control: "focus", action: { kind: "focus" } }],
    },
    { name: "disabled", cell: cell(4) },
    { name: "amber", cell: cell(5) },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      {COLUMNS.map(({ name }, index) => {
        const [x, y, width, height] = button(index);
        return (
          <CentredButton
            key={name}
            id={`button-${name}`}
            width={width}
            height={height}
            margin={{ left: x, top: y }}
            alignY={-1}
            label={name === "amber" ? "PURGE" : "PULSE"}
            font={lab.font}
            size={TEXT_BODY}
            skin={lab.skin(name === "amber" ? "amber" : "default")}
            disabled={name === "disabled"}
            control={lab.control(name)}
          />
        );
      })}
      {COLUMNS.map(({ name, caption, caption_x }) => (
        <Label
          key={name}
          id={`caption-${name}`}
          at={[caption_x, 78.5]}
          text={caption}
          font={lab.font}
          size={12.5}
          color={SHEET_CAPTION}
        />
      ))}
    </>
  ),
});
