/**
 * A tree of the application's nodes in a list frame. Each visible node is a
 * row, indented by its depth, with an expand chevron on a branch, an optional
 * icon and its label; rows of collapsed branches are not declared. Selection,
 * expansion and focus are separate: the selected row shows the row tint and
 * the accent bar of a data grid's selected row, focus lights the focused
 * row's own edge, and the chevrons expand and collapse.
 *
 * The rows are one Tab stop, entered at the selected row, or the first; Up
 * and Down move focus among visible rows, Home and End to the first and last,
 * and Enter, Space or a click selects. A group has no use for Right and Left
 * on a vertical list, so the runtime returns them to the client that sent
 * them, unhandled; the application passes them to the tree's handle, `key`.
 * Right expands a collapsed branch or moves to its first child, Left
 * collapses an expanded branch or moves to the parent; the tree follows focus
 * through its rows' focus callbacks and moves it with a client focus action,
 * which the runtime adopts for the keyboard. Pressing a chevron expands or
 * collapses without selecting and leaves focus where it is, unless the
 * collapse hides the focused row: focus then moves to the collapsed node.
 *
 * The application owns the nodes, their keys, the expansion (`expanded` or
 * `defaultExpanded`) and the selection (`value` or `defaultValue`, the
 * selected node's key). The frame scrolls once its rows outgrow it, and the
 * runtime scrolls a focused row into view. Every visible row is declared:
 * a virtual list realises items by index, so a row entity would show another
 * node whenever a branch above it opened or closed.
 */
import { useImperativeHandle, useRef, useState, type Ref } from "react";
import type { GuiPhysicalKey } from "@ipp/client";
import { Children, Entity } from "../../components.js";
import { Button } from "../../gui/controls.js";
import {
  Behavior,
  Font,
  Group,
  Layout,
  Style,
  Text,
} from "../../gui/components.js";
import { ScrollView } from "../../gui/scroll.js";
import { Skin } from "../../gui/theme.js";
import type { GuiVirtualRange } from "../../gui/callbacks.js";
import {
  GROUP_VERTICAL,
  SELECT_SINGLE,
  useChoice,
  useMountSelection,
  type Choice,
  type ChoiceProps,
} from "../choice.js";
import { GUI_KIT_ICONS } from "../icons.js";
import { useGuiKit, type GuiKitScope } from "../kit.js";
import {
  LAYOUT_COLUMN,
  LAYOUT_LEAF,
  LAYOUT_ROW,
  LAYOUT_STACK,
  Strut,
  type GuiKitLayout,
} from "../layout.js";
import { TextLine } from "../text.js";

export interface TreeNode {
  /** Stable identity, unique in the tree and part of its row's ids. */
  readonly key: string;
  readonly label: string;
  /** A glyph of the GUI font before the label, such as a Material icon. */
  readonly icon?: string;
  /** A branch's children; a node with an empty array is an empty branch. */
  readonly children?: readonly TreeNode[];
  readonly disabled?: boolean;
}

/** What the application calls on the tree. */
export interface TreeViewHandle {
  /**
   * A key the runtime returned unhandled. Right and Left act on the focused
   * row; returns whether the tree used the key, which it does only while one
   * of its rows holds focus.
   */
  key(key: GuiPhysicalKey): boolean;
}

export interface TreeViewProps extends ChoiceProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  /** Symbolic id of the frame; rows extend it with their keys. */
  readonly id: string;
  readonly nodes: readonly TreeNode[];
  /** Keys of the expanded branches, controlled. */
  readonly expanded?: readonly string[];
  readonly defaultExpanded?: readonly string[];
  readonly onExpandedChange?: (expanded: readonly string[]) => void;
  readonly ref?: Ref<TreeViewHandle>;
  readonly layout?: GuiKitLayout;
}

/** One visible row: its node, depth and neighbours in the hierarchy. */
interface VisibleRow {
  readonly node: TreeNode;
  readonly depth: number;
  readonly parent: string | undefined;
  /** Whether the node is a branch, and whether it is expanded. */
  readonly branch: boolean;
  readonly open: boolean;
}

/** The rows of `nodes` whose ancestors are all expanded, in tree order. */
function visibleRows(
  nodes: readonly TreeNode[],
  expanded: ReadonlySet<string>,
): VisibleRow[] {
  const rows: VisibleRow[] = [];
  const visit = (
    level: readonly TreeNode[],
    depth: number,
    parent?: string,
  ) => {
    for (const node of level) {
      const branch = node.children !== undefined;
      const open = branch && expanded.has(node.key);
      rows.push({ node, depth, parent, branch, open });
      if (open) visit(node.children!, depth + 1, node.key);
    }
  };
  visit(nodes, 0);
  return rows;
}

/** The tree's geometry in the World's units. */
function geometry(kit: GuiKitScope) {
  const t = kit.tokens;
  return {
    row: kit.unit(t.row),
    icon: kit.unit(t.icon),
    gap: kit.unit(t.inset / 2),
    // A child's chevron sits under its parent's icon.
    indent: kit.unit(t.icon + t.inset / 2),
    // The scroll bar's thickness and inset: while the rows scroll, the bar
    // keeps a column three bars wide, as a data grid's does.
    bar: kit.unit(t.bar),
  };
}

export function TreeView({
  id,
  layer = 0,
  nodes,
  expanded,
  defaultExpanded = [],
  onExpandedChange,
  ref,
  layout,
  ...props
}: TreeViewProps) {
  const kit = useGuiKit();
  const g = geometry(kit);
  const choice = useChoice(props);
  const [ownExpanded, setOwnExpanded] = useState(defaultExpanded);
  const open = new Set(expanded ?? ownExpanded);
  const rows = visibleRows(nodes, open);
  const byKey = new Map(rows.map((row) => [row.node.key, row]));
  // The focused row's key as the feedback stream last reported it.
  const focus = useRef<string | undefined>(undefined);
  const [range, setRange] = useState<GuiVirtualRange | null>(null);
  const scrolling = (range?.capacity[1] ?? 0) > 0;

  const setExpanded = (next: ReadonlySet<string>) => {
    const keys = [...next];
    if (expanded === undefined) setOwnExpanded(keys);
    onExpandedChange?.(keys);
  };
  const focusRow = (key: string) => choice.act(key, { kind: "focus" });
  /** Whether `key` lies below `ancestor` in the visible rows. */
  const below = (key: string | undefined, ancestor: string) => {
    for (let row = key && byKey.get(key); row; ) {
      if (row.parent === ancestor) return true;
      row = row.parent === undefined ? undefined : byKey.get(row.parent);
    }
    return false;
  };
  const toggle = (row: VisibleRow) => {
    const next = new Set(open);
    if (row.open) {
      next.delete(row.node.key);
      // Hiding the focused row would leave nothing focused.
      if (below(focus.current, row.node.key)) focusRow(row.node.key);
    } else next.add(row.node.key);
    setExpanded(next);
  };

  useImperativeHandle(ref, () => ({
    key(key) {
      const row =
        focus.current === undefined ? undefined : byKey.get(focus.current);
      if (!row || row.node.disabled) return false;
      if (key === "right") {
        if (!row.branch) return false;
        const first = row.node.children![0];
        if (!row.open) toggle(row);
        else if (first) focusRow(first.key);
        return true;
      }
      if (key === "left") {
        if (row.open) toggle(row);
        else if (row.parent !== undefined) focusRow(row.parent);
        else return false;
        return true;
      }
      return false;
    },
  }));

  const pad = g.gap;
  return (
    <Entity id={id}>
      <Style layer={layer} />
      <Layout kind={LAYOUT_STACK} {...layout} />
      <Font source={kit.font} font_size={kit.fontSize} />
      <ScrollView
        bar_thickness={scrolling ? g.bar : 0}
        bar_inset={g.bar}
        onRangeChange={setRange}
      />
      <Children>
        <Entity id={`${id}/rows`}>
          <Layout
            kind={LAYOUT_COLUMN}
            height={rows.length * g.row + 2 * pad}
            padding_top={pad}
            padding_bottom={pad}
            padding_left={pad}
            padding_right={scrolling ? 3 * g.bar : pad}
          />
          <Group axis={GROUP_VERTICAL} selection={SELECT_SINGLE} />
          <Children>
            {rows.map((row) => (
              <TreeRow
                key={row.node.key}
                id={`${id}/row/${row.node.key}`}
                row={row}
                choice={choice}
                onFocus={(focused) => {
                  if (focused) focus.current = row.node.key;
                  else if (focus.current === row.node.key)
                    focus.current = undefined;
                }}
                onToggle={() => toggle(row)}
              />
            ))}
          </Children>
        </Entity>
      </Children>
    </Entity>
  );
}

function TreeRow({
  id,
  row,
  choice,
  onFocus,
  onToggle,
}: {
  readonly id: string;
  readonly row: VisibleRow;
  readonly choice: Choice;
  readonly onFocus: (focused: boolean) => void;
  readonly onToggle: () => void;
}) {
  const kit = useGuiKit();
  const g = geometry(kit);
  const { node } = row;
  const item = choice.item(node.key);
  const selected = useMountSelection(item.selected);
  const tone = node.disabled ? "neutral" : "text";
  const color = kit.color(tone);
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_ROW}
        height={g.row}
        padding_left={g.gap + row.depth * g.indent}
        padding_right={g.gap}
      />
      <Skin theme={kit.theme("gridRow")} />
      <Behavior semantic_label={node.label} enabled={!node.disabled} />
      <Button
        label=""
        selected={selected}
        onSelectedChange={item.onSelectedChange}
        onFocusChange={(event) => onFocus(event.focused)}
        ref={item.ref}
      />
      <Children>
        <Strut id={`${id}/strut`} height={g.row} />
        {row.branch ? (
          <Entity id={`${id}/chevron`}>
            <Layout
              kind={LAYOUT_LEAF}
              width={g.icon}
              height={g.icon}
              align_y={0}
            />
            <Font source={kit.font} font_size={g.icon} />
            <Skin theme={kit.theme("treeChevron")} />
            <Behavior
              focusable={false}
              semantic_label={row.open ? "Collapse" : "Expand"}
            />
            <Button
              label={GUI_KIT_ICONS[row.open ? "expanded" : "collapsed"]}
              onPress={onToggle}
            />
          </Entity>
        ) : (
          // A leaf keeps the chevron's column, so labels of a level align.
          <Entity id={`${id}/leaf`}>
            <Layout kind={LAYOUT_LEAF} width={g.icon} height={0} />
          </Entity>
        )}
        {node.icon !== undefined && (
          <Entity id={`${id}/icon`}>
            <Layout kind={LAYOUT_LEAF} align_y={0} margin_left={g.gap} />
            <Style
              red={color[0]}
              green={color[1]}
              blue={color[2]}
              alpha={color[3]}
            />
            <Text text={node.icon} source={kit.font} font_size={g.icon} />
          </Entity>
        )}
        <TextLine
          id={`${id}/label`}
          text={node.label}
          tone={tone}
          layout={{ flex: 1, margin_left: g.gap }}
        />
      </Children>
    </Entity>
  );
}
