/** Sheet a, row 04: Slider at 65% idle, hover, dragging, focus and disabled. */
import { Behavior, Font, Slider } from "@ipp/react/gui";
import {
  At,
  CAPS_HEIGHT,
  CAPS_TOP,
  Fill,
  Label,
  SHEET_CAPTION,
  SHEET_PAGE,
} from "../kit.js";
import { defineSpecimen, type Point, type Rect } from "../specimen.js";
import { INSET, TEXT_BODY, TEXT_SMALL } from "../themes/geometry.js";
import { text as textColor } from "../themes/palette.js";

const EXTENT = [770, 92] as const;
export const VALUE = 0.65;

/**
 * Each column's rail start and vertical middle as measured on the sheet; the
 * rendered sheet drifts upward by about two units towards the disabled column.
 */
const COLUMNS = [
  { name: "idle", caption: "Idle", x: 15.5, middle: 38.25 },
  { name: "hover", caption: "Hover", x: 165.5, middle: 38.25 },
  { name: "dragging", caption: "Dragging", x: 314, middle: 37.5 },
  { name: "focus", caption: "Focus", x: 464.5, middle: 37.25 },
  { name: "disabled", caption: "Disabled", x: 614, middle: 36 },
] as const;

/**
 * The sheet's rail is 100 units long. The runtime paints a square thumb of
 * 0.75 control heights over a centred rail of 0.25, so this height gives the
 * sheet's 16-unit thumb (and a 5.3-unit rail where the sheet has 9).
 */
const WIDTH = 100;
const HEIGHT = 64 / 3;

const slider = (x: number, middle: number): Rect => [
  x,
  middle - HEIGHT / 2,
  WIDTH,
  HEIGHT,
];
/** Each column's slider rectangle, by column name. */
export const SLIDERS = Object.fromEntries(
  COLUMNS.map(({ name, x, middle }) => [name, slider(x, middle)]),
) as Readonly<Record<(typeof COLUMNS)[number]["name"], Rect>>;
const cell = (x: number): Rect => [x - 10, 0, 145, 92];

/**
 * The painted thumb centre at the committed value, as the runtime's slider
 * rail places it (`crates/ipp-core/src/world/systems/gui/local/controls/slider.rs`):
 * a press there grabs the thumb without changing the value.
 */
function thumb([x, y, width, height]: Rect): Point {
  const edge = Math.min(0.75 * height, 0.75 * width);
  return [x + edge / 2 + VALUE * (width - edge), y + height / 2];
}

export default defineSpecimen({
  extent: EXTENT,
  theme: "slider",
  reference: { image: "a04-slider.png", origin: [352, 0] },
  states: COLUMNS.map(({ name, x, middle }) => ({
    name,
    cell: cell(x),
    ...(name === "hover"
      ? { pin: [{ kind: "hover", at: thumb(slider(x, middle)) }] as const }
      : name === "dragging"
        ? { pin: [{ kind: "press", at: thumb(slider(x, middle)) }] as const }
        : name === "focus"
          ? {
              pin: [
                { kind: "action", control: name, action: { kind: "focus" } },
              ] as const,
            }
          : {}),
  })),
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      {COLUMNS.map(({ name, x, middle }) => (
        <At key={name} id={`slider-${name}`} rect={slider(x, middle)}>
          {lab.skin("default")}
          {/* The body type the design language's lengths are drawn at. */}
          <Font source={lab.font} font_size={TEXT_BODY} />
          {name === "disabled" && <Behavior enabled={false} />}
          <Slider
            min={0}
            max={1}
            step={0.01}
            value={VALUE}
            ref={lab.control(name)}
          />
        </At>
      ))}
      {COLUMNS.map(({ name, x, middle }) => (
        <Label
          key={name}
          id={`value-${name}`}
          at={[
            x + WIDTH + INSET / 2,
            middle - (CAPS_TOP + CAPS_HEIGHT / 2) * TEXT_SMALL,
          ]}
          text="65%"
          font={lab.font}
          size={TEXT_SMALL}
          color={textColor}
        />
      ))}
      {COLUMNS.map(({ name, caption, x }) => (
        <Label
          key={name}
          id={`caption-${name}`}
          at={[x, 61]}
          text={caption}
          font={lab.font}
          size={12.5}
          color={SHEET_CAPTION}
        />
      ))}
    </>
  ),
});
