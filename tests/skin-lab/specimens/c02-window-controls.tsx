/**
 * Sheet c, panel 02: window controls, which sheet b does not show, drawn with
 * the GUI kit. The SIGNAL MONITOR `Panel` with minimise, maximise and close
 * `WindowControls` in its header; the four window controls free-standing; the
 * close button idle, hovered in the amber variant, focused and disabled; and
 * the minimised panel, its title bar. Section labels are labelled separators
 * without a stub. The chart is out of scope (ipp-u0na); its area stays empty.
 *
 * The sheet hovers its close button in amber: here that cell is the amber
 * variant, a close that discards work, hovered.
 *
 * Drawn at sheet c's scale through a nested `GuiKit`, so every kit length is
 * its design size times `K`; the body's own lengths are control-sheet values
 * times `K`, and placements on the canvas follow the sheet.
 */
import { Fragment } from "react";
import { Children, Entity } from "@ipp/react";
import { Layout } from "@ipp/react/gui";
import {
  GuiKit,
  LabelledSeparator,
  Panel,
  PanelHeader,
  Separator,
  TextLine,
  WindowControl,
  WindowControls,
} from "@ipp/react/gui-kit";
import {
  COLUMN,
  FONT_LINE,
  Fill,
  Label,
  LEAF,
  ROW,
  SHEET_CAPTION,
  SHEET_PAGE,
  centre,
  placed,
} from "../kit.js";
import { SHEET_C_PANEL_SCALE as K } from "../scale.js";
import { defineSpecimen, type Rect } from "../specimen.js";
import { CONTROL_HEIGHT, DENSE_ROW, TEXT_BODY } from "../themes/geometry.js";
import { accent } from "../themes/palette.js";

/** The c02 crop, sheet c (484, 98) to (950, 562). */
const EXTENT = [466, 464] as const;

const u = (value: number) => value * K;

/** The sheet's heading, captions and description: document decoration at their drawn sizes. */
const HEADING_SIZE = 26.5;
const CAPTION_SIZE = 11.4;
const DESCRIPTION_SIZE = 12;

/** Sheet layout in canvas units. */
const SECTION: Rect = [3, 5, 461, 455.5];
const HEADING_AT = [19, 15.2] as const;
const HEADING_RULE = { at: [17.5, 47] as const, width: 432.5 };
const PANEL_AT = [25.5, 61] as const;
/** Section labels by the top of their text line, as the sheet places them. */
const SECTION_LABELS = [
  { id: "anatomy", label: "CONTROL ANATOMY", top: 192.8 },
  { id: "states", label: "CLOSE BUTTON STATES", top: 285.8 },
  { id: "minimized", label: "MINIMIZED (TITLE BAR)", top: 383.8 },
] as const;
const SECTION_LABEL_LEFT = 23.5;
const SECTION_LABEL_WIDTH = 418.5;
const DESCRIPTION_AT = [25, 441.3] as const;

/** The panel in control-sheet units: the body is a chart and a readout. */
const PANEL = { width: 446, height: 128 };
const BODY = { top: 16, height: 60, chart: 320, gap: 24 };
const READOUT = { labelTop: 4, valueTop: 8 };

/**
 * The window controls, all idle. Positions are canvas units; the caption
 * offset below each button is in control-sheet units.
 */
const ICON_BUTTON_WIDTH = 68;
const ANATOMY = {
  y: 214.9,
  captionTop: 45.7,
  buttons: [
    { kind: "minimize", caption: "Minimize", x: 36.9 },
    { kind: "maximize", caption: "Maximize", x: 146.4 },
    { kind: "restore", caption: "Restore", x: 257.9 },
    { kind: "close", caption: "Close", x: 368.9 },
  ],
} as const;

/** The close button states, in the same units. */
const STATES = {
  y: 308.4,
  captionTop: 47.3,
  buttons: [
    { name: "idle", caption: "Idle", x: 36.9 },
    { name: "hover", caption: "Hover", x: 143 },
    { name: "focus", caption: "Focus", x: 250 },
    { name: "disabled", caption: "Disabled", x: 360.4 },
  ],
} as const;

/** The minimised panel, placed in canvas units and sized in control-sheet units. */
const TITLE_BAR = { at: [24, 404] as const, width: 368 };
const MINIMIZED_CAPTION_AT = [382.5, 413.4] as const;

const stateButton = (index: number): Rect => [
  STATES.buttons[index]!.x,
  STATES.y,
  u(ICON_BUTTON_WIDTH),
  u(CONTROL_HEIGHT),
];

const STATE_CELLS: readonly Rect[] = [
  [0, 290, 119, 84],
  [119, 290, 109, 84],
  [228, 290, 108.6, 84],
  [336.6, 290, 129.4, 84],
];

/** The specimen presses nothing; the controls only need to be present. */
const none = () => {};

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "c02-window-controls.png", origin: [0, 0] },
  states: [
    { name: "panel", cell: [10, 52, 446, 136] },
    { name: "anatomy", cell: [10, 188, 446, 96] },
    { name: "idle", cell: STATE_CELLS[0]! },
    {
      name: "hover",
      cell: STATE_CELLS[1]!,
      pin: [{ kind: "hover", at: centre(stateButton(1)) }],
    },
    {
      name: "focus",
      cell: STATE_CELLS[2]!,
      pin: [{ kind: "action", control: "focus", action: { kind: "focus" } }],
    },
    { name: "disabled", cell: STATE_CELLS[3]! },
    { name: "titlebar", cell: [10, 376, 446, 84] },
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
          text="02 / WINDOW CONTROLS"
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
            <WindowControls
              id="panel/controls"
              onMinimize={none}
              onMaximize={none}
              onClose={none}
            />
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
        {SECTION_LABELS.map(({ id, label, top }) => (
          <LabelledSeparator
            key={id}
            id={`section-${id}`}
            label={label}
            stub={false}
            // The row centres the label's line where the sheet has it.
            layout={placed(
              [
                SECTION_LABEL_LEFT,
                top - u(DENSE_ROW - FONT_LINE * TEXT_BODY) / 2,
              ],
              SECTION_LABEL_WIDTH,
            )}
          />
        ))}
        {ANATOMY.buttons.map(({ kind, caption, x }) => (
          <Fragment key={kind}>
            <WindowControl
              id={`anatomy-${kind}`}
              kind={kind}
              docked={false}
              layout={placed([x, ANATOMY.y])}
            />
            <Label
              id={`anatomy-${kind}-caption`}
              at={[x, ANATOMY.y + u(ANATOMY.captionTop)]}
              width={u(ICON_BUTTON_WIDTH)}
              centred
              text={caption}
              font={lab.font}
              size={u(CAPTION_SIZE)}
              color={SHEET_CAPTION}
            />
          </Fragment>
        ))}
        {STATES.buttons.map(({ name, caption }, index) => {
          const [x, y, width] = stateButton(index);
          return (
            <Fragment key={name}>
              <WindowControl
                id={`state-${name}`}
                kind="close"
                docked={false}
                amber={name === "hover"}
                disabled={name === "disabled"}
                ref={lab.control(name)}
                layout={placed([x, y])}
              />
              <Label
                id={`state-${name}-caption`}
                at={[x, y + u(STATES.captionTop)]}
                width={width}
                centred
                text={caption}
                font={lab.font}
                size={u(CAPTION_SIZE)}
                color={SHEET_CAPTION}
              />
            </Fragment>
          );
        })}
        <Panel
          id="title-bar"
          minimized
          layout={placed(TITLE_BAR.at, u(TITLE_BAR.width))}
        >
          <PanelHeader id="title-bar/header" title="SIGNAL MONITOR">
            <WindowControls id="title-bar/controls" onMinimize={none} />
          </PanelHeader>
        </Panel>
        <Label
          id="minimized-caption"
          at={MINIMIZED_CAPTION_AT}
          text="Minimized"
          font={lab.font}
          size={u(CAPTION_SIZE)}
          color={SHEET_CAPTION}
        />
        <Label
          id="description"
          at={DESCRIPTION_AT}
          text="Application window / not OS chrome"
          font={lab.font}
          size={u(DESCRIPTION_SIZE)}
          color={SHEET_CAPTION}
        />
      </GuiKit>
    </>
  ),
});
