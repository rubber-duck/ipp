/**
 * Sheet c, panel 03: what sheet c adds to b03, drawn with the GUI kit: the
 * `DataGrid` body scrolls nine records in the grid's height, its vertical bar
 * in a column of its own after the last column, and STATUS takes the width
 * left; also the cell editor and the empty state at this sheet's scale. Nine
 * records give the sheet's thumb length; the sheet's "1-3 of 3" cannot
 * scroll.
 *
 * Drawn at the scale of sheet c's data grid section through a nested
 * `GuiKit`; placements on the canvas are sheet units measured on the crop.
 */
import { Entity } from "@ipp/react";
import { Font, Layout, TextInput } from "@ipp/react/gui";
import {
  DataGrid,
  EmptyState,
  GuiKit,
  Panel,
  PanelHeader,
  type DataGridColumn,
} from "@ipp/react/gui-kit";
import {
  Fill,
  Label,
  LEAF,
  SHEET_CAPTION,
  SHEET_PAGE,
  placed,
} from "../kit.js";
import { SHEET_C_GRID_SCALE as K } from "../scale.js";
import { defineSpecimen, type Point, type Rect } from "../specimen.js";
import {
  CONTROL_HEIGHT,
  LINE,
  SELECTION_GUTTER,
  TEXT_BODY,
} from "../themes/geometry.js";

/** The c03 crop, sheet c (955, 98) to (1442, 562). */
const EXTENT = [487, 464] as const;

const u = (value: number) => value * K;

/** The panel's outer corner and size in control-sheet units. */
const GRID_AT: Point = [62.35, 67.4];
const PANEL = [392, 224] as const;

const COLUMNS: readonly DataGridColumn[] = [
  { key: "node", title: "NODE", width: 128 },
  { key: "signal", title: "SIGNAL", width: 112, align: "end" },
  { key: "status", title: "STATUS" },
];

/** The table sits half a content inset inside the frame, as in b03. */
const TABLE_INSET = 8;
const GRID_WIDTH = PANEL[0] - 2 * TABLE_INSET + SELECTION_GUTTER;

const ROWS = [
  ["Alpha", "65%", "Online"],
  ["Bravo", "42%", "Standby"],
  ["Charlie", "88%", "Online"],
  ["Delta", "71%", "Online"],
  ["Echo", "12%", "Offline"],
  ["Foxtrot", "97%", "Online"],
  ["Golf", "54%", "Standby"],
  ["Hotel", "33%", "Online"],
  ["India", "80%", "Online"],
].map(([node, signal, status]) => ({
  key: node!.toLowerCase(),
  cells: { node: node!, signal: signal!, status: status! },
}));

const EDITOR: Rect = [62, 333.5, 176 * K, CONTROL_HEIGHT * K];
const EMPTY: Point = [272.75, 332.75];
const EMPTY_WIDTH = 188 * K;
const CAPTION_SIZE = 12.5 * K;

/** A point inside the editor to the right of its text: a click there focuses it. */
const inside = ([x, y, width, height]: Rect): Point => [
  x + width - 12,
  y + height / 2,
];

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "c03-data-grid.png", origin: [0, 0] },
  states: [
    { name: "grid", cell: [52, 57, 425, 241] },
    { name: "scrollbar", cell: [430, 95, 40, 175] },
    { name: "selection", cell: [58, 168, 400, 54] },
    {
      name: "editing",
      // The cell reaches the edge glow around the focused field.
      cell: [43, 305, 220, 88],
      // UTF-8 offsets: the caret after "42"; the pointer then leaves.
      pin: [
        { kind: "click", at: inside(EDITOR) },
        { kind: "selectText", start: 2, end: 2 },
        { kind: "hover", at: [EDITOR[0], EDITOR[1] + EDITOR[3] + 40] },
      ],
    },
    { name: "empty", cell: [262, 300, 215, 85] },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <GuiKit fontSize={TEXT_BODY * K}>
        <Panel
          id="grid-panel"
          layout={{ ...placed(GRID_AT, u(PANEL[0])), height: u(PANEL[1]) }}
        >
          <PanelHeader id="grid-panel/header" title="NODE STATUS" />
          <DataGrid
            id="grid"
            columns={COLUMNS}
            rows={ROWS}
            scroll
            sort={{ column: "signal", direction: "descending" }}
            selected="bravo"
            focusedCell={{ row: "bravo", column: "signal" }}
            footer={`1-3 of ${ROWS.length}`}
            layout={{
              width: u(GRID_WIDTH),
              flex: 1,
              margin_left: u(TABLE_INSET - SELECTION_GUTTER - LINE),
            }}
          />
        </Panel>
        <Entity id="editor">
          <Layout
            kind={LEAF}
            width={EDITOR[2]}
            height={EDITOR[3]}
            margin_left={EDITOR[0]}
            margin_top={EDITOR[1]}
            align_x={-1}
            align_y={-1}
          />
          <Font source={lab.font} font_size={TEXT_BODY * K} />
          <TextInput text="42" />
        </Entity>
        <EmptyState
          id="empty"
          text="No records"
          layout={placed(EMPTY, EMPTY_WIDTH)}
        />
      </GuiKit>
      <Label
        id="caption-editing"
        at={[65.2, 311]}
        text="Editing"
        font={lab.font}
        size={CAPTION_SIZE}
        color={SHEET_CAPTION}
      />
      <Label
        id="caption-empty"
        at={[275, 311]}
        text="No records"
        font={lab.font}
        size={CAPTION_SIZE}
        color={SHEET_CAPTION}
      />
    </>
  ),
});
