/**
 * A secondary text button, such as CLEAR, Retry or Cancel: the part cut and a
 * text label in small type at the small control height, its width hugging its
 * label with the content inset on either side. It centres vertically in a row.
 * The amber variant, for a destructive action such as Purge, swaps the accent
 * and the idle line for amber in every state, as on every button.
 */
import { Entity } from "../components.js";
import { Button } from "../gui/controls.js";
import { Behavior, Font, Layout } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import type {
  GuiFeedbackListeners,
  GuiPressListener,
} from "../gui/callbacks.js";
import type { GuiControlRef } from "../gui/control-ref.js";
import { useGuiKit } from "./kit.js";
import { LAYOUT_LEAF, type GuiKitLayout } from "./layout.js";
import { textWidth } from "./text.js";

export interface SecondaryButtonProps extends GuiFeedbackListeners {
  readonly id: string;
  readonly label: string;
  readonly onPress?: GuiPressListener;
  readonly ref?: GuiControlRef;
  readonly disabled?: boolean;
  /** The amber variant, for a destructive action. */
  readonly amber?: boolean;
  readonly layout?: GuiKitLayout;
}

export function SecondaryButton({
  id,
  label,
  onPress,
  ref,
  disabled = false,
  amber = false,
  layout,
  ...feedback
}: SecondaryButtonProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const size = kit.typeSize("small");
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_LEAF}
        width={textWidth(label, size) + kit.unit(2 * t.inset)}
        height={kit.unit(t.smallHeight)}
        align_y={0}
        {...layout}
      />
      <Font source={kit.font} font_size={size} />
      <Skin
        theme={kit.theme(amber ? "secondarySmallAmber" : "secondarySmall")}
      />
      {disabled && <Behavior enabled={false} />}
      <Button
        label={label}
        {...feedback}
        {...(onPress ? { onPress } : {})}
        {...(ref ? { ref } : {})}
      />
    </Entity>
  );
}
