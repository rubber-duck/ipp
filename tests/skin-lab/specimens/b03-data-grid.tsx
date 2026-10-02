/**
 * Sheet b, panel 03: the NODE STATUS data grid with a selected row and a
 * focused cell, drawn with the GUI kit: a `Panel` holding a `DataGrid`; then
 * the cell editor, a text input in its default look, pinned focused with the
 * caret after "42", and the kit's `EmptyState`. The annotation callouts, the
 * section frame and the dotted page are concept art.
 *
 * Drawn at sheet b's scale through a nested `GuiKit`, so every kit length is
 * its design size times `K`; placements on the canvas are sheet units
 * measured on the crop.
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
import { SHEET_B_SCALE as K } from "../scale.js";
import { defineSpecimen, type Point, type Rect } from "../specimen.js";
import {
  CONTROL_HEIGHT,
  LINE,
  SELECTION_GUTTER,
  TEXT_BODY,
} from "../themes/geometry.js";

/** The b03 crop, sheet b (1020, 200) to (1522, 842). */
const EXTENT = [502, 642] as const;

const u = (value: number) => value * K;

/** The panel's outer corner and size in control-sheet units. */
const GRID_AT: Point = [54.85, 86.25];
const PANEL = [300, 232] as const;

const COLUMNS: readonly DataGridColumn[] = [
  { key: "node", title: "NODE", width: 100 },
  { key: "signal", title: "SIGNAL", width: 96, align: "end" },
  { key: "status", title: "STATUS", width: 88 },
];

/**
 * The table's first column line sits half a content inset inside the frame
 * on either side, as on the sheet; the selection gutter lies before it.
 */
const TABLE_INSET = 8;
const GRID_WIDTH = SELECTION_GUTTER + 100 + 96 + 88;

const ROWS = [
  ["Alpha", "65%", "Online"],
  ["Bravo", "42%", "Standby"],
  ["Charlie", "88%", "Online"],
].map(([node, signal, status]) => ({
  key: node!.toLowerCase(),
  cells: { node: node!, signal: signal!, status: status! },
}));

const EDITOR: Rect = [55, 480, 136 * K, CONTROL_HEIGHT * K];
const EMPTY: Point = [278.75, 480.25];
const EMPTY_WIDTH = 140 * K;
const CAPTION_SIZE = 12.5 * K;

/** A point inside the editor to the right of its text: a click there focuses it. */
const inside = ([x, y, width, height]: Rect): Point => [
  x + width - 12,
  y + height / 2,
];

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "b03-data-grid.png", origin: [0, 0] },
  states: [
    { name: "grid", cell: [44, 76, 444, 352] },
    { name: "header", cell: [58, 132, 416, 64] },
    { name: "selection", cell: [52, 228, 426, 68] },
    {
      name: "editing",
      // The cell reaches the edge glow around the focused field.
      cell: [30, 445, 245, 117],
      // UTF-8 offsets: the caret after "42"; the pointer then leaves.
      pin: [
        { kind: "click", at: inside(EDITOR) },
        { kind: "selectText", start: 2, end: 2 },
        { kind: "hover", at: [EDITOR[0], EDITOR[1] + EDITOR[3] + 40] },
      ],
    },
    { name: "empty", cell: [268, 440, 220, 105] },
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
            sort={{ column: "signal", direction: "descending" }}
            selected="bravo"
            focusedCell={{ row: "bravo", column: "signal" }}
            footer="1-3 of 3"
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
        at={[57.2, 449.8]}
        text="Editing"
        font={lab.font}
        size={CAPTION_SIZE}
        color={SHEET_CAPTION}
      />
      <Label
        id="caption-empty"
        at={[281.7, 449.8]}
        text="Empty"
        font={lab.font}
        size={CAPTION_SIZE}
        color={SHEET_CAPTION}
      />
    </>
  ),
});
