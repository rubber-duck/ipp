/**
 * Sheet h, the tooltip, drawn with the GUI kit's `Tooltip` on the Host
 * clock: the sheet's gain field and help button, whose tooltip opens above
 * it once the button holds visible focus for the focus delay; and a help
 * button at the canvas's right edge whose tooltip, asked for on its right,
 * flips to its left once the pointer has rested on it for the hover delay.
 * The sheet repositions its second tooltip beside a section line; the
 * runtime flips at the canvas edge, so that button sits at the edge.
 *
 * Each state captures after `wait` lets the delay pass. The kit's tooltip
 * has no attachment pointer and draws small text. Drawn at the navigation
 * sheet's scale through a nested `GuiKit`.
 */
import { Children, Entity, type AssetReference } from "@ipp/react";
import {
  Button,
  Font,
  Layout,
  TextInput,
  type GuiControlRef,
} from "@ipp/react/gui";
import { GuiKit, Tooltip } from "@ipp/react/gui-kit";
import {
  FONT_ADVANCE,
  Fill,
  Label,
  SHEET_CAPTION,
  SHEET_PAGE,
  around,
} from "../kit.js";
import { SHEET_H_SCALE as K } from "../scale.js";
import { defineSpecimen, type Point, type Rect } from "../specimen.js";
import {
  CONTROL_HEIGHT,
  DENSE_ROW,
  INSET,
  TEXT_BODY,
  TEXT_SMALL,
} from "../themes/geometry.js";

/** The h03 crop is 2x of sheet h. */
const EXTENT = [770, 400] as const;

const u = (value: number) => value * K;

const TEXT = ["Gain controls signal", "strength."] as const;

/** The help buttons: the sheet's beside its field, and one at the edge. */
const BUTTONS = {
  focus: [357.5, 155, u(CONTROL_HEIGHT), u(CONTROL_HEIGHT)],
  edge: [
    770 - u(CONTROL_HEIGHT) - 8,
    287,
    u(CONTROL_HEIGHT),
    u(CONTROL_HEIGHT),
  ],
} as const satisfies Record<string, Rect>;
type Name = keyof typeof BUTTONS;

/** The tooltip's extent: its longest line in small text and its rows. */
const TIP = [
  u(TEXT[0].length * FONT_ADVANCE * TEXT_SMALL + INSET),
  u(TEXT.length * DENSE_ROW + INSET / 2),
] as const;
const GAP = u(INSET / 4);

const centre = ([x, y, width, height]: Rect): Point => [
  x + width / 2,
  y + height / 2,
];

/** A state's cell: the button and its open tooltip, with their glow. */
const CELLS: Readonly<Record<Name, Rect>> = {
  focus: around(
    [
      centre(BUTTONS.focus)[0] - TIP[0] / 2,
      BUTTONS.focus[1] - GAP - TIP[1],
      TIP[0],
      TIP[1] + GAP + BUTTONS.focus[3],
    ],
    12,
  ),
  edge: [
    BUTTONS.edge[0] - GAP - TIP[0] - 12,
    BUTTONS.edge[1] - 12,
    770 - (BUTTONS.edge[0] - GAP - TIP[0] - 12),
    Math.max(TIP[1], BUTTONS.edge[3]) + 24,
  ],
};

/** A help button with its tooltip, on `side` of it. */
function Help({
  name,
  side,
  font,
  control,
}: {
  readonly name: Name;
  readonly side: "top" | "right";
  readonly font: AssetReference;
  readonly control: GuiControlRef;
}) {
  const [x, y, width, height] = BUTTONS[name];
  return (
    <Entity id={`help-${name}`}>
      <Layout
        kind={0}
        width={width}
        height={height}
        margin_left={x}
        margin_top={y}
        align_x={-1}
        align_y={-1}
      />
      <Font source={font} font_size={u(TEXT_BODY)} />
      <Button label="?" ref={control} />
      <Children>
        <Tooltip id={`help-${name}/tip`} text={TEXT} side={side} />
      </Children>
    </Entity>
  );
}

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "h03-tooltip.png", origin: [0, 0] },
  states: [
    {
      name: "focus",
      cell: CELLS.focus,
      pin: [
        { kind: "action", control: "help-focus", action: { kind: "focus" } },
        { kind: "wait", seconds: 0.5 },
      ],
    },
    {
      name: "edge",
      cell: CELLS.edge,
      pin: [
        { kind: "hover", at: centre(BUTTONS.edge) },
        { kind: "wait", seconds: 0.6 },
      ],
    },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <Label
        id="gain-label"
        at={[26, 165]}
        text="GAIN"
        font={lab.font}
        size={u(TEXT_BODY)}
      />
      <Entity id="gain">
        <Layout
          kind={0}
          width={337.5 - 110}
          height={u(CONTROL_HEIGHT)}
          margin_left={110}
          margin_top={155}
          align_x={-1}
          align_y={-1}
        />
        <Font source={lab.font} font_size={u(TEXT_BODY)} />
        <TextInput text="65%" />
      </Entity>
      {(
        [
          ["Keyboard focus.", [432, 170]],
          ["Reposition at edge.", [432, 362]],
        ] as const
      ).map(([text, at], index) => (
        <Label
          key={text}
          id={`caption-${index}`}
          at={at}
          text={text}
          font={lab.font}
          size={u(12.5)}
          color={SHEET_CAPTION}
        />
      ))}
      <GuiKit fontSize={u(TEXT_BODY)}>
        <Help
          name="focus"
          side="top"
          font={lab.font}
          control={lab.control("help-focus")}
        />
        <Help
          name="edge"
          side="right"
          font={lab.font}
          control={lab.control("help-edge")}
        />
      </GuiKit>
    </>
  ),
});
