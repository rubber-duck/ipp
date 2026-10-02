/**
 * Sheet h, the expander, drawn with the GUI kit's `Expander`: collapsed, and
 * expanded with keyboard focus on its header and three dense content rows
 * under quiet separators, where the sheet has them; then, below the crop, a
 * hovered header and a collapsed one that a real click expands, which a
 * semantic press collapses again after its capture.
 *
 * Each expander sits in its own column, as the kit expects: the content
 * follows the header in that column. Drawn at the navigation sheet's scale
 * through a nested `GuiKit`.
 */
import { Children, Entity } from "@ipp/react";
import { Layout } from "@ipp/react/gui";
import {
  Expander,
  GuiKit,
  Row,
  Separator,
  TextLine,
  type ExpanderProps,
} from "@ipp/react/gui-kit";
import { Fill, SHEET_PAGE, centre } from "../kit.js";
import { SHEET_H_SCALE as K } from "../scale.js";
import { defineSpecimen, type Rect } from "../specimen.js";
import type { SpecimenContext } from "../specimen.js";
import {
  CONTROL_HEIGHT,
  DENSE_ROW,
  INSET,
  TEXT_BODY,
} from "../themes/geometry.js";

/** The h02 crop is 2x of sheet h (14, 556) to (738, 956); two expanders added below. */
const EXTENT = [724, 660] as const;

/** The expanders' left edge and width, and each one's top, on the sheet. */
const LEFT = 23;
const WIDTH = 671;
const TOPS = { collapsed: 100, expanded: 204, hover: 392, click: 480 } as const;
const HEADER = CONTROL_HEIGHT * K;

/** Room each column leaves for the header and three content rows. */
const COLUMN = HEADER + 3 * DENSE_ROW * K;

const header = (name: keyof typeof TOPS): Rect => [
  LEFT,
  TOPS[name],
  WIDTH,
  HEADER,
];

/** A state's cell: its column and the headers' glow. */
const cell = (name: keyof typeof TOPS, height = HEADER): Rect => [
  LEFT - 16,
  TOPS[name] - 16,
  WIDTH + 32,
  height + 32,
];

const ROWS = [
  ["Gain", "65%"],
  ["Scan", "Active"],
  ["Channel", "Render"],
] as const;

/** The sheet's content: dense rows of a name and a value, quiet lines between. */
function content(id: string) {
  return ROWS.flatMap(([name, value], index) => [
    ...(index
      ? [
          <Separator
            key={`${name}-line`}
            id={`${id}/${name}-line`}
            tone="quiet"
            layout={{
              margin_left: (INSET / 2) * K,
              width: WIDTH - INSET * K,
            }}
          />,
        ]
      : []),
    <Row
      key={name}
      id={`${id}/${name}`}
      height={DENSE_ROW}
      layout={{ padding_left: INSET * K, padding_right: INSET * K }}
    >
      <TextLine id={`${id}/${name}/name`} text={name} layout={{ flex: 1 }} />
      <TextLine
        id={`${id}/${name}/value`}
        text={value}
        layout={{ width: 120 * K }}
      />
    </Row>,
  ]);
}

/** One expander in its own column at its sheet position. */
function Column({
  name,
  lab,
  ...props
}: Omit<ExpanderProps, "id" | "label" | "children"> & {
  readonly name: keyof typeof TOPS;
  readonly lab: SpecimenContext;
}) {
  return (
    <Entity id={`column-${name}`}>
      <Layout
        kind={2}
        width={WIDTH}
        height={COLUMN}
        margin_left={LEFT}
        margin_top={TOPS[name]}
        align_x={-1}
        align_y={-1}
      />
      <Children>
        <Expander
          id={`expander-${name}`}
          label="Advanced settings"
          summary="3 options"
          ref={lab.control(name)}
          {...props}
        >
          {content(name)}
        </Expander>
      </Children>
    </Entity>
  );
}

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "h02-expander.png", origin: [0, 0] },
  states: [
    { name: "collapsed", cell: cell("collapsed") },
    {
      name: "expanded-focus",
      cell: cell("expanded", COLUMN),
      pin: [{ kind: "action", control: "expanded", action: { kind: "focus" } }],
    },
    {
      name: "hover",
      cell: cell("hover"),
      pin: [{ kind: "hover", at: centre(header("hover")) }],
    },
    {
      name: "click",
      cell: cell("click", COLUMN),
      pin: [{ kind: "click", at: centre(header("click")) }],
      restore: [
        { kind: "action", control: "click", action: { kind: "press" } },
      ],
    },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <GuiKit fontSize={TEXT_BODY * K}>
        <Column name="collapsed" lab={lab} />
        <Column name="expanded" lab={lab} expanded />
        <Column name="hover" lab={lab} />
        <Column name="click" lab={lab} />
      </GuiKit>
    </>
  ),
});
