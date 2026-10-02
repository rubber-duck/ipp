/**
 * The workbench's NODES tab: FIND, an autocomplete over the nodes' names,
 * and the station's nodes in a scrolling data grid, strongest signal first;
 * the panel's footer holds the online count, SYNC and the amber PURGE.
 * Accepting a suggestion scrolls the grid to the node and selects it. A
 * press selects a row and focuses its name cell; pressing the selected row
 * again edits the name, which Enter commits. A row's context request, a
 * secondary press or the Menu key, opens its commands: Rename, Ping and the
 * amber Remove. SYNC adds the nodes again row by row; PURGE asks for
 * confirmation in a dialog, then removes them row by row down to the empty
 * state. The 3D input shield stands in front of PURGE.
 *
 * The context menu and the dialog are roots of the canvas, which the
 * dashboard declares at its top level.
 */
import type { GuiControlHandle } from "@ipp/react/gui";
import {
  Autocomplete,
  ConfirmationDialog,
  ContextMenu,
  DataGrid,
  PanelFooter,
  SecondaryButton,
  TextLine,
  type ContextMenuState,
  type DataGridColumn,
  type MenuItem,
} from "@ipp/react/gui-kit";
import { useRef, useState } from "react";
import {
  TOKENS,
  WORKBENCH_HEIGHT,
  WORKBENCH_WIDTH,
  type Rect,
} from "./presentation.js";
import type { GuiSceneState } from "./scene.js";

/** Symbolic IDs of the grid, the footer's buttons and the overlays. */
export const NODE_FIND_ENTITY = "gui-node-find";
export const NODE_GRID_ENTITY = "gui-nodes";
export const SYNC_ENTITY = "gui-sync";
export const PURGE_ENTITY = "gui-purge";
export const NODE_MENU_ENTITY = "gui-node-menu";
export const PURGE_DIALOG_ENTITY = "gui-purge-dialog";

/** A node row's context menu, keyed by the node. */
export type NodeMenu = ContextMenuState<string>;

/** Material Design glyphs of the GUI font for the node commands. */
const PENCIL = "\u{f03eb}";
const ACCESS_POINT = "\u{f0003}";
const DELETE = "\u{f01b4}";

/**
 * The grid's columns in the tab's inset: a seven-letter name, the sorted
 * SIGNAL header with its marker and Standby each just fit their cells.
 */
export const NODE_COLUMNS: readonly DataGridColumn[] = [
  { key: "node", title: "NODE" },
  { key: "signal", title: "SIGNAL", width: 71, align: "end" },
  { key: "status", title: "STATUS", width: 77 },
];

/** The grid shows its header and six rows and scrolls the rest. */
const GRID_ROW = 36;
const GRID_HEIGHT = 7 * GRID_ROW;

/** FIND's width: the tab's content at the inset. */
const FIND_WIDTH = WORKBENCH_WIDTH - 2 * TOKENS.lineWidth - 2 * TOKENS.inset;

export function NodesTab({
  scene,
  menu,
}: {
  readonly scene: GuiSceneState;
  readonly menu: NodeMenu;
}) {
  const station = scene.station;
  const grid = useRef<GuiControlHandle | null>(null);
  const [query, setQuery] = useState("");
  const needle = query.trim().toLowerCase();
  const suggestions =
    needle === ""
      ? []
      : station.nodes
          .filter(({ name }) => name.toLowerCase().includes(needle))
          .map(({ key, name }) => ({ key, label: name }));
  // Select a found node and scroll its row to the middle of the grid,
  // which the runtime clamps to the grid's scroll range.
  const find = (key: string) => {
    const index = station.rows.findIndex((row) => row.key === key);
    if (index < 0) return;
    station.findNode(key);
    const viewport = GRID_HEIGHT - GRID_ROW;
    const offset = Math.max(0, index * GRID_ROW - (viewport - GRID_ROW) / 2);
    void grid.current
      ?.action({ kind: "scrollTo", offset: [0, offset] })
      .catch(scene.reportFailure);
  };
  return (
    <>
      <Autocomplete
        id={NODE_FIND_ENTITY}
        label="FIND"
        placeholder="FIND NODE"
        suggestions={suggestions}
        onInputChange={setQuery}
        onSelect={find}
        onCommit={() => {
          if (suggestions[0]) find(suggestions[0].key);
        }}
        layout={{ width: FIND_WIDTH }}
      />
      <DataGrid
        id={NODE_GRID_ENTITY}
        columns={NODE_COLUMNS}
        rows={station.rows}
        scroll
        scrollRef={(handle) => {
          grid.current = handle;
        }}
        sort={{ column: "signal", direction: "descending" }}
        {...(station.selected === undefined
          ? {}
          : { selected: station.selected })}
        {...(station.focusedCell ? { focusedCell: station.focusedCell } : {})}
        {...(station.editingCell ? { editingCell: station.editingCell } : {})}
        onRowPress={station.pressRow}
        onRowContextMenu={(key, event) => menu.opener(key)(event)}
        onCellEdit={station.renameNode}
        emptyText="No records"
        layout={{ height: GRID_HEIGHT, margin_top: TOKENS.inset / 2 }}
      />
    </>
  );
}

/**
 * The workbench's footer under every tab: the online count, SYNC and the
 * amber PURGE, which the input shield stands in front of.
 */
export function NodesFooter({ scene }: { readonly scene: GuiSceneState }) {
  const station = scene.station;
  return (
    <PanelFooter id="gui-nodes-footer">
      <TextLine
        id="gui-nodes-count"
        text={`${station.online}/${station.nodes.length} ONLINE`}
        layout={{ flex: 1 }}
      />
      <SecondaryButton
        id={SYNC_ENTITY}
        label="SYNC"
        disabled={station.busy}
        onPress={station.sync}
      />
      <SecondaryButton
        id={PURGE_ENTITY}
        label="PURGE"
        amber
        disabled={station.busy || station.nodes.length === 0}
        onPress={station.askPurge}
        layout={{ margin_left: TOKENS.inset / 2 }}
      />
    </PanelFooter>
  );
}

/** The commands of a node row; nothing that changes rows while busy. */
function nodeCommands(busy: boolean): readonly MenuItem[] {
  return [
    { key: "rename", label: "Rename", icon: PENCIL, disabled: busy },
    { key: "ping", label: "Ping", icon: ACCESS_POINT },
    {
      key: "remove",
      label: "Remove",
      icon: DELETE,
      tone: "amber",
      separator: true,
      disabled: busy,
    },
  ];
}

/** The context menu of the node rows, a root of the canvas. */
export function NodeContextMenu({
  scene,
  menu,
}: {
  readonly scene: GuiSceneState;
  readonly menu: NodeMenu;
}) {
  const station = scene.station;
  return (
    <ContextMenu
      id={NODE_MENU_ENTITY}
      menu={menu}
      items={nodeCommands(station.busy)}
      onSelect={(command, key) =>
        command === "rename"
          ? station.editNode(key)
          : command === "ping"
            ? station.pingNode(key)
            : station.removeNode(key)
      }
    />
  );
}

/** PURGE's confirmation, a root of the canvas on the dialog layer. */
export function PurgeDialog({ scene }: { readonly scene: GuiSceneState }) {
  const station = scene.station;
  return (
    <ConfirmationDialog
      id={PURGE_DIALOG_ENTITY}
      open={station.purgeAsked}
      title="Purge nodes?"
      body={[
        `Remove ${station.nodes.length} nodes from the table.`,
        "A toast offers to restore them.",
      ]}
      action="Purge"
      onConfirm={() => station.answerPurge(true)}
      onCancel={() => station.answerPurge(false)}
    />
  );
}

/**
 * Canvas rectangle of PURGE in a panel placed at `[x, y]`: the footer's last
 * small button, the content inset from the panel's end and centred in the
 * footer row at the panel's bottom.
 */
export function purgeRect(x: number, y: number): Rect {
  const width = 5 * 0.54 * TOKENS.textSmall + 2 * TOKENS.inset;
  const footer = TOKENS.smallHeight + TOKENS.inset;
  return [
    x + WORKBENCH_WIDTH - TOKENS.lineWidth - TOKENS.inset - width,
    y + WORKBENCH_HEIGHT - footer + (footer - TOKENS.smallHeight) / 2,
    width,
    TOKENS.smallHeight,
  ];
}
