import type { GuiPhysicalKey } from "@ipp/client";
import { createElement as h } from "react";
import {
  Children,
  Entity,
  createRoot,
  type ReactWorldClient,
} from "../../../../packages/ipp-react/src/index.js";
import { Layout } from "../../../../packages/ipp-react/src/gui.js";
import {
  GuiKit,
  RadioGroup,
  Tabs,
  TextLine,
  TreeView,
  type TreeViewHandle,
} from "../../../../packages/ipp-react/src/gui-kit.js";
import { readControl, symbols, type PanelSpec } from "./composites.js";

/**
 * The GUI kit's choice composites: a panel World whose React root declares a
 * radio group, a tab strip with its content and a tree view at half the
 * kit's design size. The application callbacks record what reaches them, and
 * the tree receives the keys the runtime returns unhandled, as an
 * application passes them on.
 */
export const KIT_CHOICE_PANEL = [192, 192] as const;

/** Half the design body size: radio options 16 high, tabs 20, rows 18. */
const FONT_SIZE = 8;
const MARGIN = 4;

/** What each composite reported to the application, in order. */
export interface KitChoiceReports {
  readonly radio: string[];
  readonly tabs: string[];
  readonly tree: string[];
  readonly expanded: (readonly string[])[];
}

export function kitChoicePanel(origin: readonly [number, number]) {
  return {
    name: "kit-choice",
    origin,
    extent: KIT_CHOICE_PANEL,
    async build({ client, font, contract, report }) {
      const [width, height] = KIT_CHOICE_PANEL;
      const reports: KitChoiceReports = {
        radio: [],
        tabs: [],
        tree: [],
        expanded: [],
      };
      const tree: { current: TreeViewHandle | null } = { current: null };
      const root = createRoot(client as unknown as ReactWorldClient, {
        onError: report,
      });
      await root.render(
        h(
          GuiKit,
          { contract, font, fontSize: FONT_SIZE },
          h(
            Entity,
            { id: "kit-root" },
            h(Layout, {
              kind: 2,
              width,
              height,
              padding_left: MARGIN,
              padding_right: MARGIN,
              padding_top: MARGIN,
              padding_bottom: MARGIN,
            }),
            h(
              Children,
              null,
              h(RadioGroup, {
                id: "kit-radio",
                options: [
                  { value: "x", label: "X" },
                  { value: "y", label: "Y" },
                  { value: "z", label: "Z", disabled: true },
                ],
                defaultValue: "x",
                horizontal: true,
                onChange: (value) => reports.radio.push(value),
              }),
              h(Tabs, {
                id: "kit-tabs",
                tabs: ["one", "two", "three"].map((value) => ({
                  value,
                  label: value.toUpperCase(),
                  content: h(TextLine, {
                    id: `kit-content-${value}`,
                    text: value,
                    size: "small",
                  }),
                })),
                defaultValue: "one",
                onChange: (value) => reports.tabs.push(value),
                layout: { height: 52, margin_top: MARGIN },
              }),
              h(TreeView, {
                id: "kit-tree",
                nodes: [
                  {
                    key: "a",
                    label: "A",
                    children: [
                      { key: "a1", label: "A1" },
                      { key: "a2", label: "A2" },
                    ],
                  },
                  { key: "b", label: "B" },
                ],
                onChange: (key) => reports.tree.push(key),
                onExpandedChange: (keys) => reports.expanded.push(keys),
                ref: tree,
                layout: { flex: 1, margin_top: MARGIN },
              }),
            ),
          ),
        ),
      );

      const control = async (symbol: string) => {
        const entity = (await symbols(client)).get(symbol);
        const state =
          entity === undefined ? undefined : await readControl(client, entity);
        if (!state) throw new Error(`Kit control ${symbol} is missing`);
        return state;
      };

      return {
        reports,
        /** A key the runtime returned unhandled, for the tree. */
        unhandledKey(key: GuiPhysicalKey) {
          tree.current?.key(key);
        },
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
        /** The Buttons among `wanted` whose `selected` field is set. */
        async selected(wanted: readonly string[]) {
          const reads = await Promise.all(wanted.map(control));
          return wanted.filter(
            (_, index) => reads[index]!.fields.selected === true,
          );
        },
        /** A control's evaluated box in the panel's canvas. */
        async bounds(symbol: string) {
          return (await control(symbol)).bounds;
        },
        async close() {
          await root.unmount();
        },
      };
    },
  } satisfies PanelSpec;
}

export type KitChoicePanel = Awaited<
  ReturnType<ReturnType<typeof kitChoicePanel>["build"]>
>;
