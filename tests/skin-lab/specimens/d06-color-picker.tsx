/**
 * Sheet d, the colour picker, drawn with the GUI kit's `ColorPicker`: the
 * runtime's colour control at the sheet's #54F4FF with the caption and the
 * rails' names over it, the R, G, B and A fields beside it, the presets and
 * the hex entry in sRGB, idle where the sheet has its picker; then, beside
 * and below it at the design language's own body size, the G field focused by
 * a click, a translucent preset pressed, which sets the colour to magenta at
 * half coverage over the checker, and a hex that does not parse, committed
 * with Enter, which keeps the colour and shows the error line.
 *
 * The kit's picker is its own arrangement: the swatch is the control's own,
 * along its bottom, and the presets and hex lie under the control rather than
 * beside it. Drawn at the value sheet's scale through a nested `GuiKit`.
 */
import {
  ColorPicker,
  GuiKit,
  parseHex,
  type ColorPreset,
} from "@ipp/react/gui-kit";
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
  INSET,
  SMALL_HEIGHT,
  TEXT_BODY,
  TEXT_SMALL,
} from "../themes/geometry.js";
import { text as textColor } from "../themes/palette.js";

/** The d06 crop is 2x of the sheet's 484 x 436 picker; states added around it. */
const EXTENT = [1024, 1024] as const;

/** The sheet's #54F4FF. */
export const CYAN = parseHex("#54F4FF")!;

/** The presets: the language's error magenta and amber, white, and magenta at half coverage. */
const PRESETS: readonly ColorPreset[] = [
  { value: parseHex("#F4449F")!, label: "Magenta" },
  { value: parseHex("#FBDC6C")!, label: "Amber" },
  { value: parseHex("#E5F5F7")!, label: "White" },
  { value: parseHex("#F4449F80")!, label: "Magenta, half coverage" },
];

/** Each picker's top-left and the scale its kit is drawn at. */
const PICKERS = {
  idle: { at: [20, 90], scale: K, caption: "Idle", captionAt: [20, 600] },
  focus: {
    at: [650, 90],
    scale: 1,
    caption: "Focus (G field)",
    captionAt: [650, 456],
  },
  preset: {
    at: [650, 500],
    scale: 1,
    caption: "Translucent preset pressed",
    captionAt: [650, 866],
  },
  invalid: {
    at: [20, 640],
    scale: 1,
    caption: "Invalid hex",
    captionAt: [400, 650],
  },
} as const satisfies Record<
  string,
  { at: Point; scale: number; caption: string; captionAt: Point }
>;
type Name = keyof typeof PICKERS;

/** The picker's design size: control, gap and channels, and its rows. */
const WIDTH = 240 + INSET + 112;
const HEIGHT =
  DENSE_ROW +
  200 +
  (INSET / 2 + SMALL_HEIGHT) +
  2 * (INSET / 2 + CONTROL_HEIGHT);

/** A point of picker `name` at design offsets from its top-left. */
const point = (name: Name, x: number, y: number): Point => {
  const { at, scale } = PICKERS[name];
  return [at[0] + x * scale, at[1] + y * scale];
};

/** The middle of a channel field: past its letter, in row `row`. */
const channelField = (name: Name, row: number) => {
  const letter = FONT_ADVANCE * TEXT_BODY + TEXT_BODY / 100;
  const left = 240 + INSET + letter + INSET / 2;
  const right = WIDTH - INSET / 2 - letter;
  return point(
    name,
    (left + right) / 2,
    DENSE_ROW + row * (CONTROL_HEIGHT + INSET / 2) + CONTROL_HEIGHT / 2,
  );
};

/** The middle of preset `index`'s swatch, after the row's caption. */
const presetSwatch = (name: Name, index: number) => {
  const caption = 7 * FONT_ADVANCE * TEXT_BODY + TEXT_BODY / 100;
  return point(
    name,
    INSET / 2 +
      caption +
      INSET / 2 +
      index * (SMALL_HEIGHT + INSET / 2) +
      SMALL_HEIGHT / 2,
    DENSE_ROW + 200 + INSET / 2 + SMALL_HEIGHT / 2,
  );
};

/** The hex field: right of "Hex", in the row under the presets. */
const hexField = (name: Name) =>
  point(
    name,
    WIDTH / 2,
    DENSE_ROW + 200 + INSET / 2 + SMALL_HEIGHT + INSET / 2 + CONTROL_HEIGHT / 2,
  );

/** A picker's cell: its box and the glow around it, apart from the others'. */
function cell(name: Name): Rect {
  const { at, scale } = PICKERS[name];
  const margin = 12;
  const left = Math.max(at[0] - margin, 0);
  const right = Math.min(at[0] + WIDTH * scale + margin, EXTENT[0]);
  const bottom = Math.min(at[1] + HEIGHT * scale + margin, EXTENT[1]);
  return [left, at[1] - margin, right - left, bottom - at[1] + margin];
}

const SMALL = TEXT_SMALL * K;
const HINT = "Select in field / adjust hue and alpha.";

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "d06-color-picker.png", origin: [0, 0] },
  states: [
    { name: "idle", cell: cell("idle") },
    {
      // A click focuses the G field with its ring; the pointer then leaves.
      name: "focus",
      cell: cell("focus"),
      pin: [
        { kind: "click", at: channelField("focus", 1) },
        { kind: "hover", at: point("focus", -40, 0) },
      ],
    },
    {
      name: "preset",
      cell: cell("preset"),
      pin: [
        { kind: "click", at: presetSwatch("preset", 3) },
        { kind: "settle" },
        { kind: "hover", at: point("preset", -40, 0) },
      ],
      restore: [
        {
          kind: "action",
          control: "preset",
          action: {
            kind: "color",
            value: [CYAN.hue, CYAN.saturation, CYAN.value, CYAN.alpha],
          },
        },
      ],
    },
    {
      // "#12G" replaces "#54F4FF" and Enter refuses it: the colour stays.
      name: "invalid",
      cell: cell("invalid"),
      pin: [
        { kind: "click", at: hexField("invalid") },
        { kind: "selectText", start: 0, end: 7 },
        { kind: "typeText", text: "#12G" },
        { kind: "key", key: "enter" },
        { kind: "settle" },
        { kind: "hover", at: point("invalid", -40, 0) },
      ],
    },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <Label
        id="hint"
        at={[484 - HINT.length * FONT_ADVANCE * SMALL, 78 - CAPS_TOP * SMALL]}
        width={HINT.length * FONT_ADVANCE * SMALL + 4}
        text={HINT}
        font={lab.font}
        size={SMALL}
        color={textColor}
      />
      {(Object.keys(PICKERS) as Name[]).map((name) => {
        const { at, scale } = PICKERS[name];
        return (
          <GuiKit key={name} fontSize={TEXT_BODY * scale}>
            <ColorPicker
              id={`picker-${name}`}
              label="COLOR"
              defaultValue={CYAN}
              presets={PRESETS}
              ref={lab.control(name)}
              layout={placed(at)}
            />
          </GuiKit>
        );
      })}
      {(Object.keys(PICKERS) as Name[]).map((name) => {
        const { caption, captionAt } = PICKERS[name];
        return (
          <Label
            key={name}
            id={`caption-${name}`}
            at={[captionAt[0], captionAt[1] - CAPS_TOP * 12.5]}
            text={caption}
            font={lab.font}
            size={12.5}
            color={SHEET_CAPTION}
          />
        );
      })}
    </>
  ),
});
