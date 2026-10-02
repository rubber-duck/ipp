/**
 * A data grid: a composition, not a control. A header row of small accent
 * labels, with the sort marker after one of them, then data rows of the row
 * height in body text, numbers aligned to the end of their cells, and a
 * footer line such as a row count. Quiet lines separate the columns and the
 * rows, so the data reads first. The grid has no frame or top line of its
 * own: in a panel, the header's division runs along its top.
 *
 * Each data row is a docked Button that paints nothing at rest: pressing it
 * calls `onRowPress` with its key, a context request on it (a secondary press,
 * the Menu key or Shift+F10) calls `onRowContextMenu` with its key and the
 * request, such as a context menu's opener, hover and focus light its square
 * edge, and
 * the selected row, which the application names, shows the row tint with the
 * accent bar in the gutter before the first column. Rows are Tab stops unless
 * `focusableRows` is false, as a grid driven by a group would want. A focused
 * cell is the focus edge on the cell's lines, and an editing cell holds a
 * text input that takes focus when it appears and reports Enter through
 * `onCellEdit`; both are the application's choice.
 *
 * The body holds every row, scrolls them in the grid's height when `scroll`
 * is set, its ScrollView handle given through `scrollRef` so the application
 * can scroll a row into view, or, given `rowCount` and `row`, realises only
 * the visible rows of a
 * virtual list; scrolling bodies keep the scroll bar in a column of its own
 * after the last column, and need a bounded height from `layout` or their
 * container. Without rows the body shows the empty state.
 */
import { useCallback, type ReactNode } from "react";
import { Children, Entity } from "../components.js";
import { Button, TextInput } from "../gui/controls.js";
import { Behavior, Font, Layout } from "../gui/components.js";
import { ScrollView, VirtualList } from "../gui/scroll.js";
import { Skin } from "../gui/theme.js";
import type { GuiContextMenuEvent } from "../gui/callbacks.js";
import type { GuiControlHandle, GuiControlRef } from "../gui/control-ref.js";
import { EmptyState } from "./empty-state.js";
import { useGuiKit, type GuiKitScope } from "./kit.js";
import {
  LAYOUT_COLUMN,
  LAYOUT_ROW,
  LAYOUT_STACK,
  Row,
  type GuiKitLayout,
} from "./layout.js";
import { Separator } from "./separator.js";
import { Icon, TextLine, textWidth } from "./text.js";

export interface DataGridColumn {
  readonly key: string;
  readonly title: string;
  /**
   * Width between the column's lines at the tokens' `em`; a column without a
   * width shares the rest by `flex`, 1 by default.
   */
  readonly width?: number;
  readonly flex?: number;
  /** Numbers align to the end of their cells; titles always start theirs. */
  readonly align?: "start" | "end";
}

export interface DataGridRow {
  readonly key: string;
  /** Cell text by column key. */
  readonly cells: Readonly<Record<string, string>>;
}

export interface DataGridCell {
  readonly row: string;
  readonly column: string;
}

export interface DataGridSort {
  readonly column: string;
  readonly direction: "ascending" | "descending";
}

interface DataGridCommonProps {
  readonly id: string;
  readonly columns: readonly DataGridColumn[];
  readonly sort?: DataGridSort;
  /** Key of the selected row. */
  readonly selected?: string;
  readonly focusedCell?: DataGridCell;
  readonly editingCell?: DataGridCell;
  readonly onRowPress?: (key: string) => void;
  /** A context request on a row: its key and the request. */
  readonly onRowContextMenu?: (key: string, event: GuiContextMenuEvent) => void;
  /** Enter in the editing cell's text input, with its text. */
  readonly onCellEdit?: (cell: DataGridCell, text: string) => void;
  /** Rows are Tab stops; true by default. */
  readonly focusableRows?: boolean;
  /** One line of small text after the table, such as `1-3 of 9`. */
  readonly footer?: string;
  /** The empty body's message; No records by default. */
  readonly emptyText?: string;
  readonly layout?: GuiKitLayout;
}

/** Every row, held in the body or, with `scroll`, scrolled in it. */
interface DataGridRowsProps {
  readonly rows: readonly DataGridRow[];
  readonly scroll?: boolean;
  /** The scrolling body's ScrollView handle, such as to scroll a row into view. */
  readonly scrollRef?: GuiControlRef;
}

/** `rowCount` rows of which only the visible ones are declared. */
interface DataGridVirtualProps {
  readonly rowCount: number;
  readonly row: (index: number) => DataGridRow;
}

export type DataGridProps = DataGridCommonProps &
  (DataGridRowsProps | DataGridVirtualProps);

/** Space between a header label and its sort marker. */
const SORT_GAP_INSETS = 1 / 4;

/** Layout fields that size a column by its width or flex. */
function columnSize(kit: GuiKitScope, column: DataGridColumn) {
  return column.width === undefined
    ? { flex: column.flex ?? 1 }
    : { width: kit.unit(column.width) };
}

/** The grid's geometry in the World's units. */
function geometry(kit: GuiKitScope, scrolling: boolean) {
  const t = kit.tokens;
  return {
    row: kit.unit(t.row),
    line: kit.unit(t.lineWidth),
    lit: kit.unit(t.litLineWidth),
    inset: kit.unit(t.inset),
    gutter: kit.unit(t.selectionGutter),
    // The scroll bar's column: the bar at its inset on either side.
    bar: scrolling ? kit.unit(3 * t.bar) : 0,
  };
}

export function DataGrid(props: DataGridProps) {
  const { id, columns, footer, emptyText = "No records", layout } = props;
  const kit = useGuiKit();
  const t = kit.tokens;
  const virtual = "rowCount" in props;
  const count = virtual ? props.rowCount : props.rows.length;
  const scrolling = count > 0 && (virtual || !!props.scroll);
  const g = geometry(kit, scrolling);
  // A body that holds its rows is as tall as they are, or as the empty state
  // in its inset, unless the grid flexes; a scrolling body takes the height
  // it is given.
  const body = scrolling
    ? undefined
    : count > 0
      ? count * g.row
      : kit.unit(t.controlHeight + 2 * t.inset);
  const footerHeight = footer === undefined ? 0 : kit.unit(t.denseRow);
  const rowProps = { grid: props, kit, g };
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_COLUMN}
        {...(body === undefined || layout?.flex
          ? {}
          : { height: g.row + body + footerHeight })}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Children>
        <Entity id={`${id}/table`}>
          <Layout kind={LAYOUT_STACK} flex={1} />
          <Children>
            <ColumnLines id={`${id}/lines`} columns={columns} kit={kit} g={g} />
            <Entity id={`${id}/header`}>
              <Layout kind={LAYOUT_STACK} height={g.row} />
              <Children>
                <Cells
                  id={`${id}/header`}
                  row={{
                    key: "",
                    cells: Object.fromEntries(
                      columns.map((column) => [column.key, column.title]),
                    ),
                  }}
                  header
                  trailing={g.bar}
                  {...rowProps}
                />
              </Children>
            </Entity>
            <Body
              id={`${id}/body`}
              count={count}
              emptyText={emptyText}
              {...rowProps}
            />
            <Separator
              id={`${id}/bottom-line`}
              tone="quiet"
              layout={{
                align_y: 1,
                margin_left: g.gutter,
                margin_right: g.bar,
              }}
            />
          </Children>
        </Entity>
        {footer !== undefined && (
          <Row
            id={`${id}/footer`}
            height={t.denseRow}
            layout={{ padding_left: g.gutter + g.inset }}
          >
            <TextLine id={`${id}/footer/text`} text={footer} size="small" />
          </Row>
        )}
      </Children>
    </Entity>
  );
}

type Geometry = ReturnType<typeof geometry>;

interface RowContext {
  readonly grid: DataGridProps;
  readonly kit: GuiKitScope;
  readonly g: Geometry;
}

/**
 * The column lines, once for the whole table: a row of the columns' sizes
 * whose every column ends in a quiet line centred on its boundary, except
 * the last unless a scroll bar's column follows.
 */
function ColumnLines({
  id,
  columns,
  kit,
  g,
}: {
  readonly id: string;
  readonly columns: readonly DataGridColumn[];
  readonly kit: GuiKitScope;
  readonly g: Geometry;
}) {
  return (
    <Entity id={id}>
      <Layout kind={LAYOUT_ROW} padding_left={g.gutter} padding_right={g.bar} />
      <Children>
        {columns.map((column, index) => (
          <Entity key={column.key} id={`${id}/${column.key}`}>
            <Layout kind={LAYOUT_ROW} {...columnSize(kit, column)} />
            {(index < columns.length - 1 || g.bar > 0) && (
              <Children>
                <Entity id={`${id}/${column.key}/space`}>
                  <Layout kind={LAYOUT_ROW} flex={1} />
                </Entity>
                <Separator
                  id={`${id}/${column.key}/line`}
                  tone="quiet"
                  vertical
                  layout={{ margin_right: -g.line / 2 }}
                />
              </Children>
            )}
          </Entity>
        ))}
      </Children>
    </Entity>
  );
}

/** The body: every row, scrolled rows, a virtual list or the empty state. */
function Body({
  id,
  count,
  emptyText,
  grid,
  kit,
  g,
}: RowContext & {
  readonly id: string;
  readonly count: number;
  readonly emptyText: string;
}) {
  // The body keeps its entity as rows come and go, and a declaration writes
  // only the fields it names, so every form of the body names its padding:
  // rows replacing the empty state must not keep its inset.
  const place = { margin_top: g.row };
  const flush = {
    padding_top: 0,
    padding_right: 0,
    padding_bottom: 0,
    padding_left: 0,
  };
  if (count === 0)
    return (
      <Entity id={id}>
        <Layout
          kind={LAYOUT_COLUMN}
          {...place}
          padding_top={g.inset}
          padding_right={g.inset}
          padding_bottom={g.inset}
          padding_left={g.gutter + g.inset}
        />
        <Children>
          <EmptyState id={`${id}/empty`} text={emptyText} />
        </Children>
      </Entity>
    );
  const bar = {
    bar_thickness: kit.unit(kit.tokens.bar),
    bar_inset: kit.unit(kit.tokens.bar),
    bar_end_inset: 0,
  };
  const rowContext = { grid, kit, g };
  if ("rowCount" in grid)
    return (
      <Entity id={id}>
        <Layout kind={LAYOUT_STACK} {...place} {...flush} />
        <Skin theme={kit.theme("gridBody")} />
        <VirtualList
          item_count={grid.rowCount}
          item_extent={g.row}
          {...bar}
          renderItem={(index) => {
            const row = grid.row(index);
            return (
              <GridRow
                id={`${grid.id}/row/${row.key}`}
                row={row}
                {...rowContext}
              />
            );
          }}
        />
      </Entity>
    );
  const rows = grid.rows.map((row) => (
    <Entity key={row.key} id={`${grid.id}/row/${row.key}`}>
      <GridRow id={`${grid.id}/row/${row.key}`} row={row} {...rowContext} />
    </Entity>
  ));
  if (!grid.scroll)
    return (
      <Entity id={id}>
        <Layout kind={LAYOUT_COLUMN} {...place} {...flush} />
        <Children>{rows}</Children>
      </Entity>
    );
  return (
    <Entity id={id}>
      <Layout kind={LAYOUT_STACK} {...place} {...flush} />
      <Skin theme={kit.theme("gridBody")} />
      <ScrollView
        {...bar}
        {...(grid.scrollRef ? { ref: grid.scrollRef } : {})}
      />
      <Children>
        <Entity id={`${id}/rows`}>
          <Layout kind={LAYOUT_COLUMN} padding_right={g.bar} />
          <Children>{rows}</Children>
        </Entity>
      </Children>
    </Entity>
  );
}

/**
 * The components of one data row's entity: a docked Button holding its cells
 * and the quiet line along its bottom.
 */
function GridRow({
  id,
  row,
  grid,
  kit,
  g,
}: RowContext & { readonly id: string; readonly row: DataGridRow }) {
  const { selected, onRowPress, onRowContextMenu, focusableRows = true } = grid;
  return (
    <>
      <Layout kind={LAYOUT_STACK} height={g.row} />
      <Skin theme={kit.theme("gridRow")} />
      {!focusableRows && <Behavior focusable={false} />}
      <Button
        label=""
        selected={row.key === selected}
        {...(onRowPress ? { onPress: () => onRowPress(row.key) } : {})}
        {...(onRowContextMenu
          ? {
              onContextMenu: (event: GuiContextMenuEvent) =>
                onRowContextMenu(row.key, event),
            }
          : {})}
      />
      <Children>
        <Cells
          id={id}
          row={row}
          // A virtual list's items span its bar's column; a scroll view's
          // rows end before it.
          trailing={"rowCount" in grid ? g.bar : 0}
          grid={grid}
          kit={kit}
          g={g}
        />
      </Children>
    </>
  );
}

/**
 * A row's cells and its bottom line. Cells are as wide as their columns, the
 * gutter before the first, and their text sits the content inset inside the
 * column lines, placed by its margins: a stack aligns its children within
 * its outer box, so padding would push end-aligned text past the line. A
 * header cell adds the sort marker, a focused cell its focus edge, and an
 * editing cell holds the editor instead of its text.
 */
function Cells({
  id,
  row,
  header = false,
  trailing,
  grid,
  kit,
  g,
}: RowContext & {
  readonly id: string;
  readonly row: DataGridRow;
  readonly header?: boolean;
  /** Space after the last column: the scroll bar's column, where the row spans it. */
  readonly trailing: number;
}) {
  const { columns, sort, focusedCell, editingCell } = grid;
  const at = (cell: DataGridCell | undefined, column: DataGridColumn) =>
    !header && cell?.row === row.key && cell.column === column.key;
  return (
    <>
      <Entity id={`${id}/cells`}>
        <Layout
          kind={LAYOUT_ROW}
          padding_left={g.gutter}
          padding_right={trailing}
          padding_bottom={g.line}
        />
        <Children>
          {columns.map((column) => {
            const cell = `${id}/${column.key}`;
            const text = row.cells[column.key] ?? "";
            const end = !header && column.align === "end";
            const sorted = header && sort?.column === column.key;
            return (
              <Entity key={column.key} id={cell}>
                <Layout kind={LAYOUT_STACK} {...columnSize(kit, column)} />
                <Children>
                  {at(editingCell, column) ? (
                    <CellEditor
                      id={`${cell}/editor`}
                      text={text}
                      onSubmit={(value) =>
                        grid.onCellEdit?.(
                          { row: row.key, column: column.key },
                          value,
                        )
                      }
                    />
                  ) : (
                    <TextLine
                      id={`${cell}/text`}
                      text={text}
                      tone={header ? "accent" : "text"}
                      size={header ? "small" : "body"}
                      layout={
                        end
                          ? { align_x: 1, margin_right: g.inset }
                          : { align_x: -1, margin_left: g.inset }
                      }
                    />
                  )}
                  {sorted && (
                    <Icon
                      id={`${cell}/sort`}
                      icon={
                        sort.direction === "ascending"
                          ? "sortAscending"
                          : "sortDescending"
                      }
                      size={kit.tokens.textBody}
                      layout={{
                        align_x: -1,
                        margin_left:
                          g.inset * (1 + SORT_GAP_INSETS) +
                          textWidth(text, kit.typeSize("small")),
                      }}
                    />
                  )}
                  {at(focusedCell, column) && (
                    // The focus edge is centred on the cell's four lines.
                    <Entity id={`${cell}/focus`}>
                      <Layout
                        kind={LAYOUT_STACK}
                        margin_left={-g.lit / 2}
                        margin_right={-g.lit / 2}
                        margin_top={-(g.line + g.lit) / 2}
                        margin_bottom={-(g.line + g.lit) / 2}
                      />
                      <Skin theme={kit.theme("cellFocus")} />
                    </Entity>
                  )}
                </Children>
              </Entity>
            );
          })}
        </Children>
      </Entity>
      <Separator
        id={`${id}/line`}
        tone="quiet"
        layout={{ align_y: 1, margin_left: g.gutter, margin_right: trailing }}
      />
    </>
  );
}

/**
 * The editing cell's text input, filling the cell between its lines in the
 * square text input look; it takes focus when its control is published.
 */
function CellEditor({
  id,
  text,
  onSubmit,
}: {
  readonly id: string;
  readonly text: string;
  readonly onSubmit: (text: string) => void;
}): ReactNode {
  const kit = useGuiKit();
  const focus = useCallback((handle: GuiControlHandle | null) => {
    void handle?.action({ kind: "focus" }).catch(() => {});
  }, []);
  return (
    <Entity id={id}>
      <Layout kind={LAYOUT_STACK} />
      <Skin theme={kit.theme("cellEditor")} />
      <TextInput
        text={text}
        ref={focus}
        onSubmit={(event) => onSubmit(event.value)}
      />
    </Entity>
  );
}
