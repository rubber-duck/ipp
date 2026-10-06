/** Sheet a, row 03: a wide Checkbox used as a switch, off, on, hover, focus and disabled on. */
import { Behavior, Checkbox, Font } from "@ipp/react/gui";
import { At, Fill, Label, SHEET_CAPTION, SHEET_PAGE, centre } from "../kit.js";
import { defineSpecimen, type Rect } from "../specimen.js";
import { SMALL_HEIGHT, TEXT_BODY } from "../themes/geometry.js";

const EXTENT = [770, 99] as const;
const COLUMNS = [
  { name: "off", caption: "Off", x: 14, checked: false },
  { name: "on", caption: "On", x: 163, checked: true },
  { name: "hover", caption: "Hover", x: 313, checked: true },
  { name: "focus", caption: "Focus", x: 461.5, checked: true },
  { name: "disabled", caption: "Disabled on", x: 613, checked: true },
] as const;

/**
 * Rails follow the sheet's written rule, 72 by 32 with a 24-unit block,
 * clearance 4 and travel 40. The sheet draws them about 124 by 37 units, so
 * they start at the drawn rail's left edge and sit on its vertical centre.
 */
const rail = (x: number): Rect => [x, 24.75, 72, SMALL_HEIGHT];
/** Each column's rail rectangle, by column name. */
export const RAILS = Object.fromEntries(
  COLUMNS.map(({ name, x }) => [name, rail(x)]),
) as Readonly<Record<(typeof COLUMNS)[number]["name"], Rect>>;
const cell = (x: number): Rect => [x - 8, 0, 136, 99];

export default defineSpecimen({
  extent: EXTENT,
  theme: "checkbox",
  reference: { image: "a03-switch.png", origin: [352, 0] },
  states: COLUMNS.map(({ name, x }) => ({
    name,
    cell: cell(x),
    ...(name === "hover"
      ? { pin: [{ kind: "hover", at: centre(rail(x)) }] as const }
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
      {COLUMNS.map(({ name, x, checked }) => (
        <At key={name} id={`switch-${name}`} rect={rail(x)}>
          {lab.skin("switch")}
          {/* The body type the design language's lengths are drawn at. */}
          <Font source={lab.font} font_size={TEXT_BODY} />
          {name === "disabled" && <Behavior enabled={false} />}
          <Checkbox label="" checked={checked} ref={lab.control(name)} />
        </At>
      ))}
      {COLUMNS.map(({ name, caption, x }) => (
        <Label
          key={name}
          id={`caption-${name}`}
          at={[x + 1, 69]}
          text={caption}
          font={lab.font}
          size={12.5}
          color={SHEET_CAPTION}
        />
      ))}
    </>
  ),
});
