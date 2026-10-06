/**
 * A menu: the command rows of a light overlay, such as a context menu or an
 * overflow menu. Each row is a Button that takes no focus, with an optional
 * icon and its label, in a group without selection, so focus stays on the
 * control that opened the menu while the group's active row, lit as a
 * hovered row, follows the pointer and the arrows. Arrows skip disabled rows
 * and separators, which are not controls; Enter, or Space outside a text
 * input, or a press activates the active row. Destructive commands take the
 * amber row, label and icon; disabled ones keep their label in the neutral
 * colour.
 *
 * Given the overlay's open state, activation closes the overlay before it
 * reports the command, and a press that arrives once the menu is closed
 * reports nothing, so a command runs once for each opening.
 *
 * The menu is as wide as its longest label needs, and at least
 * `MENU_MIN_WIDTH`; rows are the language's row height inside a half-inset
 * margin.
 */
import { Children, Entity } from "../../components.js";
import { Button } from "../../gui/controls.js";
import { Behavior, Group, Layout, Style, Text } from "../../gui/components.js";
import { Skin } from "../../gui/theme.js";
import { GROUP_VERTICAL } from "../choice.js";
import { useGuiKit, type GuiKitScope } from "../kit.js";
import {
  LAYOUT_COLUMN,
  LAYOUT_LEAF,
  LAYOUT_ROW,
  Strut,
  type GuiKitLayout,
} from "../layout.js";
import type { OverlayOpen } from "./overlay.js";
import { Separator } from "../separator.js";
import { TextLine, lineWidth } from "../text.js";

/** One command of a menu. */
export interface MenuItem {
  /** Stable key, unique in the menu and part of its row's ids. */
  readonly key: string;
  readonly label: string;
  /** A glyph of the GUI font before the label, such as a Material icon. */
  readonly icon?: string;
  /** Amber for a destructive command: its row, label and icon. */
  readonly tone?: "amber";
  readonly disabled?: boolean;
  /** A thin separator above the item, beginning a new group of commands. */
  readonly separator?: boolean;
}

export interface MenuProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  /** Symbolic id of the list; rows extend it with their keys. */
  readonly id: string;
  readonly items: readonly MenuItem[];
  /** The open state of the overlay holding the menu, which activation closes. */
  readonly overlay?: OverlayOpen;
  /** A command was activated: its key. */
  readonly onSelect?: (key: string) => void;
  readonly layout?: GuiKitLayout;
}

/** The narrowest menu, at the tokens' `em`: four control heights. */
export const MENU_MIN_WIDTH = 160;

/** The menu's geometry in the World's units. */
function geometry(kit: GuiKitScope, items: readonly MenuItem[]) {
  const t = kit.tokens;
  const gap = kit.unit(t.inset / 2);
  const icons = items.some((item) => item.icon !== undefined);
  const icon = icons ? kit.unit(t.icon) + gap : 0;
  const label = Math.max(
    0,
    ...items.map((item) => lineWidth(item.label, kit.typeSize("body"))),
  );
  // A separator is one idle line with a quarter inset either side.
  const separator = kit.unit(t.lineWidth + t.inset / 2);
  const separators = items.filter(
    (item, index) => item.separator && index > 0,
  ).length;
  return {
    gap,
    width: Math.max(
      kit.unit(MENU_MIN_WIDTH),
      // Menu margins, the row's start and end, the icon column and label.
      2 * gap + gap + icon + label + kit.unit(t.inset),
    ),
    height: 2 * gap + items.length * kit.unit(t.row) + separators * separator,
    icons,
  };
}

export function Menu({
  id,
  layer = 0,
  items,
  overlay,
  onSelect,
  layout,
}: MenuProps) {
  const kit = useGuiKit();
  const g = geometry(kit, items);
  // Separators span the rows' width between their half-inset margins.
  const separator = (layout?.width ?? g.width) - 4 * g.gap;
  const activate = (key: string) => {
    if (overlay) {
      if (!overlay.open) return;
      overlay.setOpen(false);
    }
    onSelect?.(key);
  };
  return (
    <Entity id={id}>
      <Style layer={layer} />
      <Layout
        kind={LAYOUT_COLUMN}
        width={g.width}
        height={g.height}
        padding_left={g.gap}
        padding_right={g.gap}
        padding_top={g.gap}
        padding_bottom={g.gap}
        {...layout}
      />
      <Group axis={GROUP_VERTICAL} />
      <Children>
        {items.map((item, index) => (
          <MenuRow
            key={item.key}
            id={`${id}/${item.key}`}
            item={item}
            icons={g.icons}
            divided={!!item.separator && index > 0}
            separator={separator}
            onPress={() => activate(item.key)}
          />
        ))}
      </Children>
    </Entity>
  );
}

function MenuRow({
  id,
  item,
  icons,
  divided,
  separator,
  onPress,
}: {
  readonly id: string;
  readonly item: MenuItem;
  readonly icons: boolean;
  readonly divided: boolean;
  /** A separator's width in the World's units. */
  readonly separator: number;
  readonly onPress: () => void;
}) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const gap = kit.unit(t.inset / 2);
  const tone = item.disabled ? "neutral" : (item.tone ?? "text");
  const color = kit.color(tone);
  return (
    <>
      {divided && (
        <Separator
          id={`${id}/separator`}
          layout={{
            width: separator,
            margin_top: kit.unit(t.inset / 4),
            margin_bottom: kit.unit(t.inset / 4),
            margin_left: gap,
          }}
        />
      )}
      <Entity id={id}>
        <Layout
          kind={LAYOUT_ROW}
          height={kit.unit(t.row)}
          padding_left={gap}
          padding_right={kit.unit(t.inset)}
        />
        <Skin
          theme={kit.theme(item.tone === "amber" ? "menuRowAmber" : "menuRow")}
        />
        <Behavior
          focusable={false}
          enabled={!item.disabled}
          semantic_label={item.label}
        />
        <Button label="" onPress={onPress} />
        <Children>
          <Strut id={`${id}/strut`} height={kit.unit(t.row)} />
          {icons && (
            <Entity id={`${id}/icon`}>
              <Layout kind={LAYOUT_LEAF} width={kit.unit(t.icon)} align_y={0} />
              <Style
                red={color[0]}
                green={color[1]}
                blue={color[2]}
                alpha={color[3]}
              />
              {item.icon !== undefined && (
                <Text
                  text={item.icon}
                  source={kit.font}
                  font_size={kit.unit(t.icon)}
                />
              )}
            </Entity>
          )}
          <TextLine
            id={`${id}/label`}
            text={item.label}
            tone={tone}
            layout={{ flex: 1, ...(icons ? { margin_left: gap } : {}) }}
          />
        </Children>
      </Entity>
    </>
  );
}
