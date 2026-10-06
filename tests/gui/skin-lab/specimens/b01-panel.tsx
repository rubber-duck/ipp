/**
 * Sheet b, panel 01: the TELEMETRY panel inside its section frame, drawn with
 * the GUI kit: a `Panel` with its header and docked close button, a body of
 * two text columns divided by a vertical separator, a labelled separator, the
 * events and a footer with CLEAR. The annotation callouts and the dotted page
 * are concept art.
 *
 * Drawn at sheet b's scale through a nested `GuiKit`, so every kit length is
 * its design size times `K`; the body's own lengths are control-sheet values
 * times `K`, and placements on the canvas follow the sheet.
 */
import { Children, Entity } from "@ipp/react";
import { Layout } from "@ipp/react/gui";
import {
  GuiKit,
  LabelledSeparator,
  Panel,
  PanelFooter,
  PanelHeader,
  SecondaryButton,
  Separator,
  TextLine,
  WindowControl,
} from "@ipp/react/gui-kit";
import {
  COLUMN,
  FONT_LINE,
  Fill,
  Label,
  ROW,
  SHEET_PAGE,
  placed,
} from "../kit.js";
import { SHEET_B_SCALE as K } from "../scale.js";
import { defineSpecimen, type Rect } from "../specimen.js";
import { INSET, TEXT_BODY } from "../themes/geometry.js";
import { accent } from "../themes/palette.js";

/** The b01 crop, sheet b (14, 200) to (516, 842). */
const EXTENT = [502, 642] as const;

const u = (value: number) => value * K;

/** The sheet's section heading, document decoration at its drawn size. */
const HEADING_SIZE = 20.5;

/** The section frame, its heading and the panel's placement, in canvas units. */
const SECTION: Rect = [3, 5, 495, 633];
const HEADING_AT = [20, 18] as const;
const HEADING_RULE = { at: [20, 60.5] as const, width: 453 };
const PANEL_AT = [24, 85] as const;

/**
 * The panel in control-sheet units, top to bottom after the header: the body
 * of two columns, the labelled separator, the events, then the footer at the
 * bottom.
 */
const PANEL = { width: 228, height: 296 };
const BODY = { top: 16, height: 68, column: 88, gap: 24, textTop: 8 };
const LINE_PITCH = 28;
/** Above the labelled separator and the events, placing them as the sheet does. */
const LABELLED_TOP = 17;
const EVENTS_TOP = 9;

/** The space between two body lines' boxes. */
const LINE_GAP = LINE_PITCH - FONT_LINE * TEXT_BODY;

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "b01-panel-separators.png", origin: [0, 0] },
  states: [
    { name: "panel", cell: [0, 0, EXTENT[0], EXTENT[1]] },
    { name: "header", cell: [14, 75, 340, 110] },
    { name: "body", cell: [14, 180, 340, 240] },
    { name: "footer", cell: [14, 410, 340, 100] },
  ],
  render: (lab) => {
    const lines = (id: string, first: string, second: string) => [
      <TextLine key="first" id={`${id}/first`} text={first} />,
      <TextLine
        key="second"
        id={`${id}/second`}
        text={second}
        layout={{ margin_top: u(LINE_GAP) }}
      />,
    ];
    return (
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
            text="01 / PANEL + SEPARATORS"
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
            <PanelHeader id="panel/header" title="TELEMETRY">
              <WindowControl id="panel/close" kind="close" />
            </PanelHeader>
            <Entity id="panel/body">
              <Layout
                kind={ROW}
                height={u(BODY.height)}
                margin_top={u(BODY.top)}
                padding_left={u(INSET)}
              />
              <Children>
                <Entity id="panel/names">
                  <Layout
                    kind={COLUMN}
                    width={u(BODY.column)}
                    padding_top={u(BODY.textTop)}
                  />
                  <Children>{lines("panel/names", "SIGNAL", "SCAN")}</Children>
                </Entity>
                <Separator id="panel/body/separator" vertical />
                <Entity id="panel/values">
                  <Layout
                    kind={COLUMN}
                    flex={1}
                    padding_top={u(BODY.textTop)}
                    padding_left={u(BODY.gap)}
                  />
                  <Children>{lines("panel/values", "65%", "ACTIVE")}</Children>
                </Entity>
              </Children>
            </Entity>
            <LabelledSeparator
              id="panel/events-separator"
              label="EVENTS"
              layout={{ margin_top: u(LABELLED_TOP) }}
            />
            <Entity id="panel/events">
              <Layout
                kind={COLUMN}
                flex={1}
                margin_top={u(EVENTS_TOP)}
                padding_left={u(INSET)}
              />
              <Children>
                {lines("panel/events", "PULSE SENT", "GAIN UPDATED")}
              </Children>
            </Entity>
            <PanelFooter id="panel/footer">
              <TextLine id="panel/count" text="2 EVENTS" layout={{ flex: 1 }} />
              <SecondaryButton id="panel/clear" label="CLEAR" />
            </PanelFooter>
          </Panel>
        </GuiKit>
      </>
    );
  },
});
