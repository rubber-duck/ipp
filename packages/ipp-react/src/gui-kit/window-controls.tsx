/**
 * Window controls: secondary buttons labelled with the Material outline glyph
 * of their action, in the text colour. Docked in a panel header or title bar
 * they are docked buttons, square because they sit on the container's lines;
 * free-standing they take the part cut at the control height. The amber
 * variant, for a destructive action such as a close that discards work, swaps
 * the accent and the idle line for amber in every state, as on every button.
 */
import { Children, Entity } from "../components.js";
import { Button } from "../gui/controls.js";
import { Behavior, Font, Layout } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import type {
  GuiFeedbackListeners,
  GuiPressListener,
} from "../gui/callbacks.js";
import type { GuiControlRef } from "../gui/control-ref.js";
import { GUI_KIT_ICONS } from "./icons.js";
import { useGuiKit } from "./kit.js";
import { LAYOUT_LEAF, LAYOUT_ROW, type GuiKitLayout } from "./layout.js";

export type WindowControlKind = "minimize" | "maximize" | "restore" | "close";

/** Width of a free-standing icon button, as the sheets draw it. */
const FREE_WIDTH = 68;

/** Each control's semantic label. */
const LABELS: Readonly<Record<WindowControlKind, string>> = {
  minimize: "Minimize",
  maximize: "Maximize",
  restore: "Restore",
  close: "Close",
};

export interface WindowControlProps extends GuiFeedbackListeners {
  readonly id: string;
  readonly kind: WindowControlKind;
  readonly onPress?: GuiPressListener;
  readonly ref?: GuiControlRef;
  readonly disabled?: boolean;
  /** The amber variant, for a destructive action. */
  readonly amber?: boolean;
  /** Docked in a header strip or title bar; free-standing when false. */
  readonly docked?: boolean;
  /**
   * A Tab stop, as by default; pointer-only when false, such as the close
   * button of an overlay whose Escape closes it.
   */
  readonly focusable?: boolean;
  /** Semantic label; the action's English name by default. */
  readonly label?: string;
  readonly layout?: GuiKitLayout;
}

export function WindowControl({
  id,
  kind,
  onPress,
  ref,
  disabled = false,
  amber = false,
  docked = true,
  focusable = true,
  label = LABELS[kind],
  layout,
  ...feedback
}: WindowControlProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const theme = docked
    ? amber
      ? "dockedIconAmber"
      : "dockedIcon"
    : amber
      ? "secondaryIconAmber"
      : "secondaryIcon";
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_LEAF}
        width={kit.unit(docked ? t.dockedWidth : FREE_WIDTH)}
        height={kit.unit(docked ? t.dockedHeight : t.controlHeight)}
        align_y={0}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.unit(t.icon)} />
      <Skin theme={kit.theme(theme)} />
      <Behavior
        semantic_label={label}
        enabled={!disabled}
        {...(focusable ? {} : { focusable: false })}
      />
      <Button
        label={GUI_KIT_ICONS[kind]}
        {...feedback}
        {...(onPress ? { onPress } : {})}
        {...(ref ? { ref } : {})}
      />
    </Entity>
  );
}

export interface WindowControlsProps {
  readonly id: string;
  /** Each given callback adds its control, in this order, half an inset apart. */
  readonly onMinimize?: GuiPressListener;
  readonly onMaximize?: GuiPressListener;
  readonly onRestore?: GuiPressListener;
  readonly onClose?: GuiPressListener;
  readonly layout?: GuiKitLayout;
}

/**
 * The docked window controls an application asks for, for a panel header:
 * minimise, maximise, restore and close, each present when its callback is.
 */
export function WindowControls({
  id,
  onMinimize,
  onMaximize,
  onRestore,
  onClose,
  layout,
}: WindowControlsProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const controls = (
    [
      ["minimize", onMinimize],
      ["maximize", onMaximize],
      ["restore", onRestore],
      ["close", onClose],
    ] as const
  ).filter(([, onPress]) => onPress);
  const gap = t.inset / 2;
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_ROW}
        width={kit.unit(
          controls.length * t.dockedWidth +
            Math.max(controls.length - 1, 0) * gap,
        )}
        height={kit.unit(t.dockedHeight)}
        align_y={0}
        {...layout}
      />
      <Children>
        {controls.map(([kind, onPress], index) => (
          <WindowControl
            key={kind}
            id={`${id}/${kind}`}
            kind={kind}
            onPress={onPress!}
            {...(index > 0 ? { layout: { margin_left: kit.unit(gap) } } : {})}
          />
        ))}
      </Children>
    </Entity>
  );
}
