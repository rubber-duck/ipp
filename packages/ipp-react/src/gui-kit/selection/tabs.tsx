/**
 * Tabs: a strip of tabs over a content area that shows the selected tab's
 * content, declared only while its tab is selected. The strip is one Tab
 * stop, entered at the selected tab; arrows move focus along it without
 * selecting, and Enter, Space or a click activates the focused tab, which
 * selects it. Focus is the lit contour of the focused tab; selection is the
 * accent label and the accent bar on the strip's line, so a selected tab
 * that does not hold focus reads as selected rather than focused.
 *
 * Tabs hug their labels. When they do not fit, the strip scrolls: scroll
 * buttons appear at its ends, docked on its lines and pointer-only, each
 * scrolling half the strip's width towards its end, and the runtime scrolls
 * a tab that takes focus into view. With `overflowMenu`, a docked More
 * button follows them while the strip overflows, also pointer-only, whose
 * menu lists every tab and selects the one activated, scrolling it into
 * view; keyboard users reach every tab with the arrows. Whatever `more`
 * declares follows at the strip's end.
 *
 * The component is a column of the strip, the control height, the panel
 * division along the strip's bottom and the content area, which fills the
 * rest of its height at the content inset.
 */
import { useRef, useState, type ReactNode } from "react";
import { Children, Entity } from "../../components.js";
import { Button } from "../../gui/controls.js";
import { Style, Behavior, Font, Group, Layout } from "../../gui/components.js";
import { ScrollView } from "../../gui/scroll.js";
import { Skin } from "../../gui/theme.js";
import type { GuiVirtualRange } from "../../gui/callbacks.js";
import type { GuiControlHandle } from "../../gui/control-ref.js";
import {
  GROUP_HORIZONTAL,
  SELECT_SINGLE,
  useChoice,
  useMountSelection,
  type Choice,
  type ChoiceProps,
} from "../choice.js";
import { GUI_KIT_ICONS } from "../icons.js";
import { useGuiKit } from "../kit.js";
import {
  LAYOUT_COLUMN,
  LAYOUT_LEAF,
  LAYOUT_ROW,
  type GuiKitLayout,
} from "../layout.js";
import { Menu } from "../overlays/menu.js";
import { Floating, useOverlayOpen } from "../overlays/overlay.js";
import { Separator } from "../separator.js";
import { lineWidth } from "../text.js";

export interface TabItem {
  /** The tab's value, unique in its strip and part of its entities' ids. */
  readonly value: string;
  readonly label: string;
  readonly disabled?: boolean;
  /** Entity declarations of the content area while this tab is selected. */
  readonly content?: ReactNode;
}

export interface TabsProps extends ChoiceProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  /** Symbolic id of the root column; the strip and tabs extend it. */
  readonly id: string;
  readonly tabs: readonly TabItem[];
  /**
   * While the strip overflows, a docked More button after its scroll
   * buttons whose menu lists the tabs and selects one.
   */
  readonly overflowMenu?: boolean;
  /** Entity declarations at the strip's end, after its buttons. */
  readonly more?: ReactNode;
  readonly layout?: GuiKitLayout;
}

/** `GuiScrollView.axis`: horizontal. */
const SCROLL_HORIZONTAL = 0;

export function Tabs({
  id,
  layer = 0,
  tabs,
  overflowMenu = false,
  more,
  layout,
  ...props
}: TabsProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const choice = useChoice(props);
  const height = kit.unit(t.controlHeight);
  const body = kit.typeSize("body");
  const widths = tabs.map(
    (tab) => lineWidth(tab.label, body) + kit.unit(2 * t.inset),
  );
  const strip = useRef<GuiControlHandle>(null);
  const [range, setRange] = useState<GuiVirtualRange | null>(null);
  const [offset, setOffset] = useState(0);
  const capacity = range?.capacity[0] ?? 0;
  const overflowing = capacity > 0;
  const scroll = (direction: -1 | 1) => () =>
    void strip.current
      ?.action({
        kind: "scrollBy",
        delta: [(direction * (range?.viewport[0] ?? 0)) / 2, 0],
      })
      .catch(() => {});
  const content = tabs.find((tab) => tab.value === choice.current)?.content;
  // Select a tab from the More menu and scroll the least that shows it.
  const pick = (value: string) => {
    choice.select(value);
    const index = tabs.findIndex((tab) => tab.value === value);
    const start = widths.slice(0, index).reduce((sum, w) => sum + w, 0);
    const end = start + (widths[index] ?? 0);
    const viewport = range?.viewport[0] ?? 0;
    const target =
      start < offset ? start : end > offset + viewport ? end - viewport : null;
    if (target !== null)
      void strip.current
        ?.action({ kind: "scrollTo", offset: [target, 0] })
        .catch(() => {});
  };
  return (
    <Entity id={id}>
      <Style layer={layer} />
      <Layout kind={LAYOUT_COLUMN} {...layout} />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Children>
        <Entity id={`${id}/strip`}>
          <Layout kind={LAYOUT_ROW} height={height} />
          <Children>
            {overflowing && (
              <ScrollButton
                id={`${id}/previous`}
                direction="previous"
                disabled={offset <= 0}
                onPress={scroll(-1)}
              />
            )}
            <Entity id={`${id}/view`}>
              <Layout kind={LAYOUT_LEAF} flex={1} height={height} />
              <Skin theme={kit.theme("gridBody")} />
              <ScrollView
                axis={SCROLL_HORIZONTAL}
                bar_thickness={0}
                ref={strip}
                onRangeChange={setRange}
                onScroll={(event) => setOffset(event.value.offset[0])}
              />
              <Children>
                <Entity id={`${id}/tabs`}>
                  <Layout
                    kind={LAYOUT_ROW}
                    width={widths.reduce((sum, width) => sum + width, 0)}
                    height={height}
                  />
                  <Group axis={GROUP_HORIZONTAL} selection={SELECT_SINGLE} />
                  <Children>
                    {tabs.map((tab, index) => (
                      <Tab
                        key={tab.value}
                        id={`${id}/tab/${tab.value}`}
                        tab={tab}
                        width={widths[index]!}
                        choice={choice}
                      />
                    ))}
                  </Children>
                </Entity>
              </Children>
            </Entity>
            {overflowing && (
              <ScrollButton
                id={`${id}/next`}
                direction="next"
                disabled={offset >= capacity}
                onPress={scroll(1)}
              />
            )}
            {overflowing && overflowMenu && (
              <MoreMenu id={`${id}/more`} tabs={tabs} onPick={pick} />
            )}
            {more}
          </Children>
        </Entity>
        <Separator id={`${id}/line`} />
        <Entity id={`${id}/content`}>
          <Layout
            kind={LAYOUT_COLUMN}
            flex={1}
            padding_left={kit.unit(t.inset)}
            padding_right={kit.unit(t.inset)}
            padding_top={kit.unit(t.inset)}
          />
          {content !== undefined && <Children>{content}</Children>}
        </Entity>
      </Children>
    </Entity>
  );
}

function Tab({
  id,
  tab,
  width,
  choice,
}: {
  readonly id: string;
  readonly tab: TabItem;
  readonly width: number;
  readonly choice: Choice;
}) {
  const kit = useGuiKit();
  const item = choice.item(tab.value);
  const selected = useMountSelection(item.selected);
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_LEAF}
        width={width}
        height={kit.unit(kit.tokens.controlHeight)}
      />
      <Skin theme={kit.theme("tab")} />
      <Behavior enabled={!tab.disabled} />
      <Button
        label={tab.label}
        selected={selected}
        onSelectedChange={item.onSelectedChange}
        ref={item.ref}
      />
    </Entity>
  );
}

/** A docked chevron button at one end of the strip; it never takes focus. */
function ScrollButton({
  id,
  direction,
  disabled,
  onPress,
}: {
  readonly id: string;
  readonly direction: "previous" | "next";
  readonly disabled: boolean;
  readonly onPress: () => void;
}) {
  const kit = useGuiKit();
  const t = kit.tokens;
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_LEAF}
        width={kit.unit(t.dockedWidth)}
        height={kit.unit(t.controlHeight)}
      />
      <Font source={kit.font} font_size={kit.unit(t.icon)} />
      <Skin theme={kit.theme("dockedIcon")} />
      <Behavior
        focusable={false}
        enabled={!disabled}
        semantic_label={
          direction === "previous" ? "Previous tabs" : "Next tabs"
        }
      />
      <Button label={GUI_KIT_ICONS[direction]} onPress={onPress} />
    </Entity>
  );
}

/**
 * The docked More button and its light menu of the tabs, opened below it and
 * aligned with its end; it never takes focus.
 */
function MoreMenu({
  id,
  tabs,
  onPick,
}: {
  readonly id: string;
  readonly tabs: readonly TabItem[];
  readonly onPick: (value: string) => void;
}) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const overlay = useOverlayOpen();
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_LEAF}
        width={kit.unit(t.dockedWidth)}
        height={kit.unit(t.controlHeight)}
      />
      <Font source={kit.font} font_size={kit.unit(t.icon)} />
      <Skin theme={kit.theme("dockedIcon")} />
      <Behavior focusable={false} semantic_label="More tabs" />
      <Button
        label={GUI_KIT_ICONS.expanded}
        selected={overlay.open}
        onPress={overlay.toggle}
      />
      <Children>
        <Floating
          id={`${id}/menu`}
          mode="light"
          align="end"
          open={overlay.open}
          onVisibleChange={overlay.onVisibleChange}
        >
          <Menu
            id={`${id}/items`}
            items={tabs.map((tab) => ({
              key: tab.value,
              label: tab.label,
              ...(tab.disabled ? { disabled: true } : {}),
            }))}
            overlay={overlay}
            onSelect={onPick}
          />
        </Floating>
      </Children>
    </Entity>
  );
}
