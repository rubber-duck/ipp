import { createElement as h, useState } from "react";
import {
  Children,
  Entity,
  createRoot,
  type ReactWorldClient,
} from "../../../packages/ipp-react/src/index.js";
import {
  Behavior,
  Button,
  Layout,
} from "../../../packages/ipp-react/src/gui.js";
import {
  Autocomplete,
  Dropdown,
  GuiKit,
  MultiSelect,
  SearchableDropdown,
  type SelectOption,
} from "../../../packages/ipp-react/src/gui-kit.js";
import {
  componentFields,
  readControl,
  symbols,
  type PanelSpec,
} from "./page.js";

/**
 * The GUI kit's selection controls: a panel World whose React root declares
 * a dropdown, a searchable dropdown, a multi-select and an autocomplete in a
 * column at half the kit's design size, and beside them a plain button that
 * outside presses must not reach. The application callbacks record what
 * reaches them; the autocomplete's application suggests the places that
 * start with the typed text.
 */
export const KIT_SELECT_PANEL = [192, 192] as const;

/** Half the design body size: triggers and fields 20 high, rows 18. */
const FONT_SIZE = 8;
const MARGIN = 4;
const COLUMN = 120;

const SKINS: readonly SelectOption[] = [
  { key: "aurora", label: "Aurora" },
  { key: "ember", label: "Ember", disabled: true },
  { key: "neon", label: "Neon" },
  { key: "pulse", label: "Pulse" },
];

const NODES: readonly SelectOption[] = [
  { key: "alpha", label: "Alpha Station" },
  { key: "beta", label: "Beta Relay" },
  { key: "gamma", label: "Gamma Dock" },
  { key: "pulse", label: "Pulse Array" },
];

const CHANNELS: readonly SelectOption[] = [
  { key: "render", label: "Render" },
  { key: "physics", label: "Physics" },
  { key: "network", label: "Network" },
];

const PLACES: readonly SelectOption[] = [
  { key: "alpha", label: "Alpha Station" },
  { key: "alpine", label: "Alpine Relay" },
  { key: "beta", label: "Beta Relay" },
];

/** The row symbols of the searchable dropdown's options. */
export const SEARCH_ROWS = NODES.map(
  (node) => `sel-search/options/${node.key}`,
);

/** What each control reported to the application, in order. */
export interface SelectReports {
  readonly dropdown: string[];
  readonly searchable: string[];
  readonly multi: (readonly string[])[];
  readonly input: string[];
  readonly select: string[];
  readonly commit: string[];
  /** Presses that reached the button beside the controls. */
  beneath: number;
}

/**
 * The panel's observable state, as `expectSelect` compares it, beside what
 * reached the application.
 */
export interface SelectSnapshot extends SelectReports {
  /** The focused entity's symbol and whether it shows its ring. */
  readonly focus: string | null;
  readonly focusVisible: boolean | null;
  /** The active item's symbol. */
  readonly active: string | null;
  /** The open lists, by symbol. */
  readonly open: string[];
  /** The searchable dropdown's declared rows. */
  readonly searchRows: string[];
  /** Text of the triggers' values and of the fields. */
  readonly dropdownText: string;
  readonly multiText: string;
  readonly searchText: string;
  readonly autoText: string;
}

/** The autocomplete's application. */
function Destination({ reports }: { readonly reports: SelectReports }) {
  const [text, setText] = useState("");
  const typed = text.trim().toLowerCase();
  return h(Autocomplete, {
    id: "sel-auto",
    label: "Destination",
    suggestions:
      typed === ""
        ? []
        : PLACES.filter((place) => place.label.toLowerCase().startsWith(typed)),
    onInputChange: (value) => {
      reports.input.push(value);
      setText(value);
    },
    onSelect: (key) => reports.select.push(key),
    onCommit: (value) => reports.commit.push(value),
    layout: { width: COLUMN, margin_top: MARGIN },
  });
}

export function kitSelectPanel(origin: readonly [number, number]) {
  return {
    name: "kit-select",
    origin,
    extent: KIT_SELECT_PANEL,
    async build({ client, font, contract, report }) {
      const [width, height] = KIT_SELECT_PANEL;
      const reports: SelectReports = {
        dropdown: [],
        searchable: [],
        multi: [],
        input: [],
        select: [],
        commit: [],
        beneath: 0,
      };
      const root = createRoot(client as unknown as ReactWorldClient, {
        onError: report,
      });
      const below = { width: COLUMN, margin_top: MARGIN };
      await root.render(
        h(
          GuiKit,
          { contract, font, fontSize: FONT_SIZE },
          h(
            Entity,
            { id: "select-root" },
            h(Layout, {
              kind: 1,
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
              h(
                Entity,
                { id: "select-column" },
                h(Layout, { kind: 2, width: COLUMN }),
                h(
                  Children,
                  null,
                  h(Dropdown, {
                    id: "sel-dropdown",
                    label: "Skin",
                    options: SKINS,
                    defaultValue: "aurora",
                    onChange: (key) => reports.dropdown.push(key),
                    layout: { width: COLUMN },
                  }),
                  h(SearchableDropdown, {
                    id: "sel-search",
                    label: "Node",
                    options: NODES,
                    defaultValue: "alpha",
                    onChange: (key) => reports.searchable.push(key),
                    layout: below,
                  }),
                  h(MultiSelect, {
                    id: "sel-multi",
                    label: "Channels",
                    options: CHANNELS,
                    onChange: (keys) => reports.multi.push(keys),
                    layout: below,
                  }),
                  h(Destination, { reports }),
                ),
              ),
              h(
                Entity,
                { id: "sel-beneath" },
                h(Layout, {
                  kind: 0,
                  width: width - COLUMN - 3 * MARGIN,
                  height: 20,
                  margin_left: MARGIN,
                }),
                h(Behavior, { semantic_label: "Beneath" }),
                h(Button, {
                  label: "",
                  onPress: () => {
                    reports.beneath++;
                  },
                }),
              ),
            ),
          ),
        ),
      );

      const symbolOf = (ids: Map<string, bigint>, entity: bigint) =>
        [...ids].find(([, id]) => id === entity)?.[0] ?? String(entity);
      const LISTS = [
        "sel-dropdown/list",
        "sel-search/list",
        "sel-multi/list",
        "sel-auto/list",
      ] as const;
      const TEXTS = {
        dropdownText: ["sel-dropdown/value", "CanvasText"],
        multiText: ["sel-multi/value", "CanvasText"],
        searchText: ["sel-search/search/field", "GuiTextInput"],
        autoText: ["sel-auto", "GuiTextInput"],
      } as const;
      /** One component's fields of each declared entity among `wanted`. */
      const fieldsOf = async (
        ids: Map<string, bigint>,
        wanted: readonly (readonly [string, string])[],
      ) =>
        Promise.all(
          wanted.map(async ([symbol, component]) => {
            const entity = ids.get(symbol);
            if (entity === undefined) return undefined;
            const page = await client.inspectPage({
              collection: "entities",
              target: entity,
              limit: 1,
            });
            return componentFields(client, page, entity, component);
          }),
        );

      return {
        reports,
        /** The entity `symbol` names. */
        async entity(symbol: string) {
          const found = (await symbols(client)).get(symbol);
          if (found === undefined)
            throw new Error(`Select entity ${symbol} is missing`);
          return found;
        },
        /** The panel's observable state now. */
        async snapshot(): Promise<SelectSnapshot> {
          const ids = await symbols(client);
          const [focusPage, activePage, lists, texts] = await Promise.all([
            client.inspectPage({ collection: "guiFocus" }),
            client.inspectPage({ collection: "guiActiveItems" }),
            fieldsOf(
              ids,
              LISTS.map((list) => [list, "GuiBehavior"] as const),
            ),
            fieldsOf(ids, Object.values(TEXTS)),
          ]);
          const [focus] = focusPage.guiFocus ?? [];
          const active = (activePage.guiActiveItems ?? []).map((record) =>
            symbolOf(ids, record.target.entity),
          );
          const text = (index: number, field: string) =>
            String(texts[index]?.[field] ?? "");
          return {
            focus: focus ? symbolOf(ids, focus.target.entity) : null,
            focusVisible: focus ? focus.visible : null,
            active: active.length === 1 ? active[0]! : active.join(",") || null,
            open: LISTS.filter((_, index) => lists[index]?.visible === true),
            searchRows: SEARCH_ROWS.filter((row) => ids.has(row)),
            dropdownText: text(0, "text"),
            multiText: text(1, "text"),
            searchText: text(2, "text"),
            autoText: text(3, "text"),
            dropdown: [...reports.dropdown],
            searchable: [...reports.searchable],
            multi: [...reports.multi],
            input: [...reports.input],
            select: [...reports.select],
            commit: [...reports.commit],
            beneath: reports.beneath,
          };
        },
        /** A control's evaluated box in the panel's canvas. */
        async bounds(symbol: string) {
          const entity = (await symbols(client)).get(symbol);
          const state =
            entity === undefined
              ? undefined
              : await readControl(client, entity);
          if (!state) throw new Error(`Select control ${symbol} is missing`);
          return state.bounds;
        },
        async close() {
          await root.unmount();
        },
      };
    },
  } satisfies PanelSpec;
}

export type KitSelectPanel = Awaited<
  ReturnType<ReturnType<typeof kitSelectPanel>["build"]>
>;
