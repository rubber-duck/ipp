import { createElement as h, useState } from "react";
import {
  Children,
  Entity,
  createRoot,
  type ReactWorldClient,
} from "../../../packages/ipp-react/src/index.js";
import { Button, Font, Layout } from "../../../packages/ipp-react/src/gui.js";
import {
  ConfirmationDialog,
  ContextMenu,
  GuiKit,
  Popover,
  Tooltip,
  useContextMenu,
  type MenuItem,
} from "../../../packages/ipp-react/src/gui-kit.js";
import {
  overlayVisible,
  readControl,
  symbols,
  type PanelSpec,
} from "./page.js";

/**
 * The GUI kit's overlays: a panel World whose React root declares, at half
 * the kit's design size, a target whose context menu holds a command, a
 * disabled one and a destructive one; a help button with a tooltip; a button
 * that opens a confirmation dialog; and a popover whose content is one
 * button. The application callbacks record what reaches them: commands with
 * their target, the dialog's answers, the popover's open state and presses
 * on the target.
 */
export const KIT_OVERLAYS_PANEL = [192, 192] as const;

/** Half the design body size: controls 20 high, menu rows 18. */
const FONT_SIZE = 8;

/** Controls' boxes in the panel: left, top, width, height. */
const BOXES = {
  target: [4, 4, 80, 20],
  help: [100, 4, 20, 20],
  opener: [4, 150, 80, 20],
  popover: [100, 150, 60, 16],
} as const;

const COMMANDS: readonly MenuItem[] = [
  { key: "inspect", label: "Inspect" },
  { key: "unavailable", label: "Unavailable", disabled: true },
  { key: "delete", label: "Delete", tone: "amber", separator: true },
];

/** What the application heard, in order. */
export interface KitOverlayReports {
  readonly commands: string[];
  readonly dialog: string[];
  readonly popover: boolean[];
  readonly targetPresses: string[];
}

const place = (box: readonly [number, number, number, number]) => ({
  kind: 0,
  width: box[2],
  height: box[3],
  margin_left: box[0],
  margin_top: box[1],
  align_x: -1,
  align_y: -1,
});

function Overlays({
  font,
  reports,
}: {
  readonly font: string;
  readonly reports: KitOverlayReports;
}) {
  const menu = useContextMenu<string>();
  const [dialog, setDialog] = useState(false);
  return h(
    Entity,
    { id: "ov-root" },
    h(Layout, {
      kind: 3,
      width: KIT_OVERLAYS_PANEL[0],
      height: KIT_OVERLAYS_PANEL[1],
    }),
    h(Font, { source: font, font_size: FONT_SIZE }),
    h(
      Children,
      null,
      h(
        Entity,
        { id: "ov-target" },
        h(Layout, place(BOXES.target)),
        h(Button, {
          label: "TARGET",
          onPress: () => reports.targetPresses.push("target"),
          onContextMenu: menu.opener("target"),
        }),
        // The menu is a root of the canvas at the request's point.
        h(ContextMenu<string>, {
          id: "ov-menu",
          menu,
          items: COMMANDS,
          onSelect: (key, target) => reports.commands.push(`${key}:${target}`),
        }),
      ),
      h(
        Entity,
        { id: "ov-help" },
        h(Layout, place(BOXES.help)),
        h(Button, { label: "?" }),
        h(
          Children,
          null,
          h(Tooltip, { id: "ov-help/tip", text: "Help", side: "bottom" }),
        ),
      ),
      h(
        Entity,
        { id: "ov-opener" },
        h(Layout, place(BOXES.opener)),
        h(Button, { label: "DELETE", onPress: () => setDialog(true) }),
        h(ConfirmationDialog, {
          id: "ov-dialog",
          open: dialog,
          title: "Delete node?",
          body: "Delete Cube?",
          action: "Delete",
          onConfirm: () => {
            reports.dialog.push("confirm");
            setDialog(false);
          },
          onCancel: () => {
            reports.dialog.push("cancel");
            setDialog(false);
          },
        }),
      ),
      h(
        Popover,
        {
          id: "ov-popover",
          label: "OPTS",
          title: "Options",
          width: 120,
          onOpenChange: (open) => reports.popover.push(open),
          layout: place(BOXES.popover),
        },
        h(
          Entity,
          { id: "ov-pop-ok" },
          h(Layout, { kind: 0, width: 88, height: 20 }),
          h(Button, { label: "OK" }),
        ),
      ),
    ),
  );
}

export function kitOverlaysPanel(origin: readonly [number, number]) {
  return {
    name: "kit-overlays",
    origin,
    extent: KIT_OVERLAYS_PANEL,
    async build({ client, font, contract, report }) {
      const reports: KitOverlayReports = {
        commands: [],
        dialog: [],
        popover: [],
        targetPresses: [],
      };
      const root = createRoot(client as unknown as ReactWorldClient, {
        onError: report,
      });
      await root.render(
        h(
          GuiKit,
          { contract, font, fontSize: FONT_SIZE },
          h(Overlays, { font, reports }),
        ),
      );
      const entity = async (symbol: string) => {
        const found = (await symbols(client)).get(symbol);
        if (found === undefined)
          throw new Error(`Kit entity ${symbol} is missing`);
        return found;
      };

      return {
        reports,
        entity,
        /** Which of `wanted` are declared. */
        async declared(wanted: readonly string[]) {
          const present = await symbols(client);
          return wanted.filter((symbol) => present.has(symbol));
        },
        /** The control the World focuses, with its ring, or null. */
        async focus() {
          const [page, present] = await Promise.all([
            client.inspectPage({ collection: "guiFocus" }),
            symbols(client),
          ]);
          const [record] = page.guiFocus ?? [];
          if (!record) return null;
          const symbol = [...present].find(
            ([, id]) => id === record.target.entity,
          )?.[0];
          return {
            symbol: symbol ?? String(record.target.entity),
            visible: record.visible,
          };
        },
        /** The symbols of the World's groups' active items. */
        async active() {
          const [page, present] = await Promise.all([
            client.inspectPage({ collection: "guiActiveItems" }),
            symbols(client),
          ]);
          return (page.guiActiveItems ?? []).map(
            (record) =>
              [...present].find(([, id]) => id === record.target.entity)?.[0] ??
              String(record.target.entity),
          );
        },
        /** Whether the overlay `symbol` is declared and its `visible` holds. */
        async open(symbol: string) {
          const id = (await symbols(client)).get(symbol);
          return id !== undefined && overlayVisible(client, id);
        },
        /** A control's evaluated box in the panel's canvas. */
        async bounds(symbol: string) {
          const state = await readControl(client, await entity(symbol));
          if (!state) throw new Error(`Kit control ${symbol} is missing`);
          return state.bounds;
        },
        async close() {
          await root.unmount();
        },
      };
    },
  } satisfies PanelSpec;
}

export type KitOverlaysPanel = Awaited<
  ReturnType<ReturnType<typeof kitOverlaysPanel>["build"]>
>;
