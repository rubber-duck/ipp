/**
 * Sheet b, panel 02: the close control, drawn with the GUI kit. The SIGNAL
 * MONITOR `Panel` with its header close button, and the free-standing close
 * `WindowControl` in idle, hover, pressed, focus and disabled. The chart
 * inside the panel is out of scope (charts, ipp-u0na); its area stays empty.
 *
 * Drawn at sheet b's scale through a nested `GuiKit`, so every kit length is
 * its design size times `K`; the body's own lengths are control-sheet values
 * times `K`, and placements on the canvas follow the sheet.
 */
import { Children, Entity } from "@ipp/react";
import { Layout } from "@ipp/react/gui";
import {
  GuiKit,
  Panel,
  PanelHeader,
  Separator,
  TextLine,
  WindowControl,
} from "@ipp/react/gui-kit";
import {
  COLUMN,
  Fill,
  Label,
  LEAF,
  ROW,
  SHEET_CAPTION,
  SHEET_PAGE,
  centre,
  placed,
} from "../kit.js";
import { SHEET_B_SCALE as K } from "../scale.js";
import { defineSpecimen, type Rect } from "../specimen.js";
import { CONTROL_HEIGHT, INSET, TEXT_BODY } from "../themes/geometry.js";
import { accent } from "../themes/palette.js";

/** The b02 crop, sheet b (520, 200) to (1014, 842). */
const EXTENT = [494, 642] as const;

const u = (value: number) => value * K;

/** The sheet's heading and captions, document decoration at their drawn sizes. */
const HEADING_SIZE = 20.5;
const CAPTION_SIZE = 12.5;

/** Sheet layout in canvas units. */
const SECTION: Rect = [4.6, 5.6, 485.8, 632.3];
const HEADING_AT = [21.5, 18] as const;
const HEADING_RULE = { at: [21, 60.5] as const, width: 453 };
const PANEL_AT = [26.1, 85.6] as const;
const DESCRIPTION_AT = [29, 294.8] as const;
const DESCRIPTION_PITCH = 28;

/** The panel in control-sheet units: the body is a chart and a readout. */
const PANEL = { width: 316, height: 132 };
const BODY = { top: 12, height: 60, chart: 236, gap: INSET };
const READOUT = { labelTop: 8, valueTop: 4 };

/**
 * The close states: free-standing window controls, 68 wide at the control
 * height in control-sheet units, placed in canvas units.
 */
const ICON_BUTTON_WIDTH = 68;
const BUTTONS = [
  { name: "idle", caption: "Idle", at: [58.6, 373.1] },
  { name: "hover", caption: "Hover", at: [198.6, 373.1] },
  { name: "pressed", caption: "Pressed", at: [341.5, 373.1] },
  { name: "focus", caption: "Focus", at: [117.1, 504.1] },
  { name: "disabled", caption: "Disabled", at: [278.6, 504.1] },
] as const;
/** Caption line top below its button's top edge. */
const CAPTION_TOP = 49.4;

const button = (index: number): Rect => {
  const [x, y] = BUTTONS[index]!.at;
  return [x, y, u(ICON_BUTTON_WIDTH), u(CONTROL_HEIGHT)];
};

/** Cells reach halfway to the neighbouring buttons, so each glow stays in its cell. */
const CELLS: readonly Rect[] = [
  [0, 340, 176.5, 128],
  [176.5, 340, 141.5, 128],
  [318, 340, 176, 128],
  [0, 470, 246, 130],
  [246, 470, 248, 130],
];

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "b02-close-control.png", origin: [0, 0] },
  states: [
    { name: "panel", cell: [14, 75, 470, 205] },
    { name: "idle", cell: CELLS[0]! },
    {
      name: "hover",
      cell: CELLS[1]!,
      pin: [{ kind: "hover", at: centre(button(1)) }],
    },
    {
      name: "pressed",
      cell: CELLS[2]!,
      pin: [{ kind: "press", at: centre(button(2)) }],
    },
    {
      name: "focus",
      cell: CELLS[3]!,
      pin: [{ kind: "action", control: "focus", action: { kind: "focus" } }],
    },
    { name: "disabled", cell: CELLS[4]! },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <GuiKit fontSize={TEXT_BODY * K}>
        <Panel
          id="section"
          layout={{
            ...placed([SECTION[0], SECTION[1]], SECTION[2]),
            height: SECTION[3],
          }}
        />
        <Label
          id="heading"
          at={HEADING_AT}
          text="02 / CLOSE CONTROL"
          font={lab.font}
          size={u(HEADING_SIZE)}
          color={accent}
        />
        <Separator
          id="heading-rule"
          tone="rule"
          layout={placed(HEADING_RULE.at, HEADING_RULE.width)}
        />
        <Panel
          id="panel"
          layout={{
            ...placed(PANEL_AT, u(PANEL.width)),
            height: u(PANEL.height),
          }}
        >
          <PanelHeader id="panel/header" title="SIGNAL MONITOR">
            <WindowControl id="panel/close" kind="close" />
          </PanelHeader>
          <Entity id="panel/body">
            <Layout
              kind={ROW}
              height={u(BODY.height)}
              margin_top={u(BODY.top)}
            />
            <Children>
              <Entity id="panel/chart">
                <Layout kind={LEAF} width={u(BODY.chart)} />
              </Entity>
              <Separator id="panel/body/separator" vertical />
              <Entity id="panel/readout">
                <Layout kind={COLUMN} flex={1} padding_left={u(BODY.gap)} />
                <Children>
                  <TextLine
                    id="panel/readout/label"
                    text="GAIN"
                    tone="accent"
                    size="small"
                    layout={{ margin_top: u(READOUT.labelTop) }}
                  />
                  <TextLine
                    id="panel/readout/value"
                    text="65%"
                    size="display"
                    layout={{ margin_top: u(READOUT.valueTop) }}
                  />
                </Children>
              </Entity>
            </Children>
          </Entity>
        </Panel>
        {["Close panel", "One action / no desktop chrome"].map(
          (line, index) => (
            <Label
              key={line}
              id={`description-${index}`}
              at={[
                DESCRIPTION_AT[0],
                DESCRIPTION_AT[1] + index * DESCRIPTION_PITCH,
              ]}
              text={line}
              font={lab.font}
              size={u(CAPTION_SIZE)}
              color={SHEET_CAPTION}
            />
          ),
        )}
        {BUTTONS.map(({ name }, index) => {
          const [x, y] = button(index);
          return (
            <WindowControl
              key={name}
              id={`close-${name}`}
              kind="close"
              docked={false}
              disabled={name === "disabled"}
              ref={lab.control(name)}
              layout={placed([x, y])}
            />
          );
        })}
        {BUTTONS.map(({ name, caption }, index) => {
          const [x, y, width] = button(index);
          return (
            <Label
              key={name}
              id={`caption-${name}`}
              at={[x, y + u(CAPTION_TOP)]}
              width={width}
              centred
              text={caption}
              font={lab.font}
              size={u(CAPTION_SIZE)}
              color={SHEET_CAPTION}
            />
          );
        })}
      </GuiKit>
    </>
  ),
});
