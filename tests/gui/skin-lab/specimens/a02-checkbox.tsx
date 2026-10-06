/** Sheet a, row 02: Checkbox unchecked, checked, hover, focus and disabled checked. */
import { Behavior, Checkbox, Font } from "@ipp/react/gui";
import { At, Fill, Label, SHEET_CAPTION, SHEET_PAGE, centre } from "../kit.js";
import { defineSpecimen, type Rect } from "../specimen.js";
import { SMALL_HEIGHT, TEXT_BODY } from "../themes/geometry.js";

const EXTENT = [770, 92] as const;
/** Box centres follow the sheet, whose columns are not evenly spaced. */
const COLUMNS = [
  { name: "unchecked", caption: "Unchecked", x: 81.5, checked: false },
  { name: "checked", caption: "Checked", x: 219.75, checked: true },
  { name: "hover", caption: "Hover", x: 359.5, checked: true },
  { name: "focus", caption: "Focus", x: 506.75, checked: true },
  { name: "disabled", caption: "Disabled checked", x: 650, checked: true },
] as const;

/** Small-control boxes centred where the sheet centres its boxes (y 37.5). */
const box = (x: number): Rect => [
  x - SMALL_HEIGHT / 2,
  37.5 - SMALL_HEIGHT / 2,
  SMALL_HEIGHT,
  SMALL_HEIGHT,
];
/** Each column's box rectangle, by column name. */
export const BOXES = Object.fromEntries(
  COLUMNS.map(({ name, x }) => [name, box(x)]),
) as Readonly<Record<(typeof COLUMNS)[number]["name"], Rect>>;
const cell = (x: number): Rect => [x - 65, 0, 130, 92];

export default defineSpecimen({
  extent: EXTENT,
  theme: "checkbox",
  reference: { image: "a02-checkbox.png", origin: [352, 0] },
  states: COLUMNS.map(({ name, x }) => ({
    name,
    cell: cell(x),
    ...(name === "hover"
      ? { pin: [{ kind: "hover", at: centre(box(x)) }] as const }
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
        <At key={name} id={`checkbox-${name}`} rect={box(x)}>
          {lab.skin("checkbox")}
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
          at={[x - 65, 63]}
          width={130}
          centred
          text={caption}
          font={lab.font}
          size={12.5}
          color={SHEET_CAPTION}
        />
      ))}
    </>
  ),
});
