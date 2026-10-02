/**
 * An option list: the rows of a dropdown, searchable dropdown, multi-select
 * or autocomplete, drawn on their floating surface. Each option is a Button
 * that takes no focus, in a group without selection, so focus stays on the
 * trigger or field while the group's active row, lit as a hovered row,
 * follows the pointer and the arrows; arrows skip disabled options, which
 * keep their label in the neutral tone, and Enter, or Space outside a text
 * input, or a press picks the active row. The runtime scrolls the active
 * row into view.
 *
 * The application's selection shows in one of two marks: a single selection
 * takes the bar and tint of a data grid's or tree's selected row, and
 * options that toggle take the check mark at the start of the row. A list
 * without options shows its empty row, such as No results, and a loading
 * list ends with the spinner's row. Rows are the language's row height
 * inside a half-inset margin, as a menu's; the list is as tall as its rows
 * up to `maxRows`, and scrolls past that or where its overlay has less room.
 */
import { useState } from "react";
import { Children, Entity } from "../components.js";
import { Button } from "../gui/controls.js";
import { Behavior, Group, Layout } from "../gui/components.js";
import { ScrollView } from "../gui/scroll.js";
import { Skin } from "../gui/theme.js";
import type { GuiVirtualRange } from "../gui/callbacks.js";
import { GROUP_VERTICAL } from "./choice.js";
import { useGuiKit, type GuiKitScope } from "./kit.js";
import {
  LAYOUT_COLUMN,
  LAYOUT_ROW,
  LAYOUT_STACK,
  Row,
  Strut,
  type GuiKitLayout,
} from "./layout.js";
import { Spinner } from "./spinner.js";
import { TextLine } from "./text.js";

/** One option of a selection control. */
export interface SelectOption {
  /** Stable key, unique in the list and part of its row's ids. */
  readonly key: string;
  readonly label: string;
  readonly disabled?: boolean;
}

/** How the selected options show: the selected row's bar, or check marks. */
export type OptionListMark = "selected" | "check";

export interface OptionListProps {
  /** Symbolic id of the list; rows extend it with their keys. */
  readonly id: string;
  readonly options: readonly SelectOption[];
  /** Keys of the selected options, shown with `mark`. */
  readonly selected?: readonly string[];
  /** The selected row's bar and tint by default. */
  readonly mark?: OptionListMark;
  /** An option was picked: pressed, or activated as the active row. */
  readonly onPick?: (key: string) => void;
  /** The empty row's text, shown while there are no options. */
  readonly emptyLabel?: string;
  /** Show the loading row after the options, with this text. */
  readonly loading?: string;
  /** Rows shown before the list scrolls; six by default. */
  readonly maxRows?: number;
  readonly layout?: GuiKitLayout;
}

/** Rows a list shows before it scrolls. */
export const OPTION_LIST_ROWS = 6;

/** The list's geometry in the World's units, for `rows` rows. */
export function optionListGeometry(
  kit: GuiKitScope,
  rows: number,
  maxRows = OPTION_LIST_ROWS,
) {
  const t = kit.tokens;
  const gap = kit.unit(t.inset / 2);
  const row = kit.unit(t.row);
  return {
    gap,
    row,
    // The scroll bar's thickness and inset: while the rows scroll, the bar
    // keeps a column three bars wide, as a tree's does.
    bar: kit.unit(t.bar),
    content: rows * row + 2 * gap,
    height: Math.min(rows, Math.max(1, maxRows)) * row + 2 * gap,
  };
}

export function OptionList({
  id,
  options,
  selected = [],
  mark = "selected",
  onPick,
  emptyLabel = "No options",
  loading,
  maxRows = OPTION_LIST_ROWS,
  layout,
}: OptionListProps) {
  const kit = useGuiKit();
  const empty = options.length === 0 && loading === undefined;
  const rows = options.length + (loading !== undefined || empty ? 1 : 0);
  const g = optionListGeometry(kit, rows, maxRows);
  const [range, setRange] = useState<GuiVirtualRange | null>(null);
  const scrolling = (range?.capacity[1] ?? 0) > 0;
  const chosen = new Set(selected);
  // A status row's text starts where an option's label does.
  const status = { padding_left: g.gap, padding_right: g.gap };
  return (
    <Entity id={id}>
      <Layout kind={LAYOUT_STACK} height={g.height} {...layout} />
      <Skin theme={kit.theme("gridBody")} />
      <ScrollView
        bar_thickness={scrolling ? g.bar : 0}
        bar_inset={g.bar}
        onRangeChange={setRange}
      />
      <Children>
        <Entity id={`${id}/rows`}>
          <Layout
            kind={LAYOUT_COLUMN}
            height={g.content}
            padding_top={g.gap}
            padding_bottom={g.gap}
            padding_left={g.gap}
            padding_right={scrolling ? 3 * g.bar : g.gap}
          />
          <Group axis={GROUP_VERTICAL} />
          <Children>
            {options.map((option) => (
              <OptionRow
                key={option.key}
                id={`${id}/${option.key}`}
                option={option}
                mark={mark}
                selected={chosen.has(option.key)}
                onPress={() => onPick?.(option.key)}
              />
            ))}
            {empty && (
              <Row id={`${id}/empty`} layout={status}>
                <TextLine
                  id={`${id}/empty/label`}
                  text={emptyLabel}
                  tone="neutral"
                />
              </Row>
            )}
            {loading !== undefined && (
              <Row id={`${id}/loading`} layout={status}>
                <Spinner
                  id={`${id}/loading/spinner`}
                  label={loading}
                  layout={{ align_y: 0 }}
                />
              </Row>
            )}
          </Children>
        </Entity>
      </Children>
    </Entity>
  );
}

function OptionRow({
  id,
  option,
  mark,
  selected,
  onPress,
}: {
  readonly id: string;
  readonly option: SelectOption;
  readonly mark: OptionListMark;
  readonly selected: boolean;
  readonly onPress: () => void;
}) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const gap = kit.unit(t.inset / 2);
  const row = kit.unit(t.row);
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_ROW}
        height={row}
        // A check mark takes the row's leading square, the Button's icon.
        padding_left={mark === "check" ? row : gap}
        padding_right={kit.unit(t.inset)}
      />
      <Skin
        theme={kit.theme(mark === "check" ? "optionCheckRow" : "menuRow")}
      />
      <Behavior
        focusable={false}
        enabled={!option.disabled}
        semantic_label={option.label}
      />
      <Button label="" selected={selected} onPress={onPress} />
      <Children>
        <Strut id={`${id}/strut`} height={row} />
        <TextLine
          id={`${id}/label`}
          text={option.label}
          tone={option.disabled ? "neutral" : "text"}
          layout={{ flex: 1, clip: true }}
        />
      </Children>
    </Entity>
  );
}
