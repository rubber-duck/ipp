/**
 * The trigger of a dropdown, searchable dropdown or multi-select: a field of
 * the control height, the frame cut and idle line of a text field, showing
 * its value, or its placeholder in the neutral tone, at the content inset and
 * a chevron at its end. It is a Button: a press, or Enter or Space while it
 * holds focus, toggles its list. While the list is open it takes the open
 * look of an invoker, its checked variant: the lit edge, which focus, the
 * full glow, still outshines, and its chevron points up.
 *
 * The list is the trigger's child, an anchored light overlay below it as
 * wide as the trigger, a quarter inset away, flipping above it where the
 * canvas has more room there.
 */
import { useCallback, type ReactNode, type Ref, type RefObject } from "react";
import { Children, Entity } from "../components.js";
import { Button } from "../gui/controls.js";
import { Style, Behavior, Font, Layout } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import type {
  GuiPressListener,
  GuiVisibleChangeListener,
} from "../gui/callbacks.js";
import type { GuiControlHandle, GuiControlRef } from "../gui/control-ref.js";
import { useGuiKit, type GuiKitScope } from "./kit.js";
import { LAYOUT_LEAF, LAYOUT_ROW, Strut, type GuiKitLayout } from "./layout.js";
import { Floating } from "./overlay.js";
import { TextLine } from "./text.js";

/** A trigger's width by default, at the tokens' `em`: six control heights. */
export const SELECT_WIDTH = 240;

/** Side of the chevron's square at the tokens' `em`. */
const CHEVRON = 16;

export interface SelectTriggerProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  readonly id: string;
  /** The value shown, or the placeholder while `placeholder` is set. */
  readonly text: string;
  readonly placeholder?: boolean;
  /** Whether its list is open: the open look and an upward chevron. */
  readonly open: boolean;
  readonly onPress?: GuiPressListener;
  readonly disabled?: boolean;
  /** The trigger's semantic name, such as the field's label. */
  readonly label?: string;
  readonly ref?: GuiControlRef;
  readonly layout?: GuiKitLayout;
  /** Its list: a `SelectList`. */
  readonly children?: ReactNode;
}

/**
 * The room for the text of a trigger laid out with `layout`, in the World's
 * units: its explicit width, or the default, less its insets and chevron.
 */
export function selectTextRoom(
  kit: GuiKitScope,
  layout: GuiKitLayout | undefined,
): number {
  const t = kit.tokens;
  const width = layout?.width ?? kit.unit(SELECT_WIDTH);
  return width - kit.unit(2 * t.inset + CHEVRON + t.inset / 2);
}

export function SelectTrigger({
  id,
  layer = 0,
  text,
  placeholder = false,
  open,
  onPress,
  disabled = false,
  label,
  ref,
  layout,
  children,
}: SelectTriggerProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const height = kit.unit(t.controlHeight);
  const inset = kit.unit(t.inset);
  return (
    <Entity id={id}>
      <Style layer={layer} />
      <Layout
        kind={LAYOUT_ROW}
        width={kit.unit(SELECT_WIDTH)}
        height={height}
        padding_left={inset}
        padding_right={inset}
        align_y={0}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Skin theme={kit.theme("selectTrigger")} />
      <Behavior
        enabled={!disabled}
        semantic_label={label === undefined ? text : `${label}: ${text}`}
      />
      <Button
        label=""
        selected={open}
        {...(onPress ? { onPress } : {})}
        {...(ref ? { ref } : {})}
      />
      <Children>
        <Strut id={`${id}/strut`} height={height} />
        <TextLine
          id={`${id}/value`}
          text={text}
          tone={disabled || placeholder ? "neutral" : "text"}
          layout={{ flex: 1, clip: true }}
        />
        <Entity id={`${id}/chevron`}>
          <Layout
            kind={LAYOUT_LEAF}
            width={kit.unit(CHEVRON)}
            height={kit.unit(CHEVRON)}
            margin_left={kit.unit(t.inset / 2)}
            align_y={0}
          />
          <Skin
            theme={kit.theme(
              disabled
                ? "selectChevronDisabled"
                : open
                  ? "selectChevronOpen"
                  : "selectChevron",
            )}
          />
        </Entity>
        {children}
      </Children>
    </Entity>
  );
}

export interface SelectListProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  readonly id: string;
  /** Its `Behavior.visible` as declared: open, with anything to show. */
  readonly open: boolean;
  readonly onVisibleChange?: GuiVisibleChangeListener;
  readonly children?: ReactNode;
}

/**
 * The list of a trigger or field: a light floating surface below its
 * parent, stretched to the parent's width a quarter inset away. Its content
 * stays declared while it is closed, so the runtime can return focus from
 * it.
 */
export function SelectList({
  id,
  layer = 0,
  open,
  onVisibleChange,
  children,
}: SelectListProps) {
  const kit = useGuiKit();
  return (
    <Floating
      id={id}
      layer={layer}
      mode="light"
      align="stretch"
      open={open}
      {...(onVisibleChange ? { onVisibleChange } : {})}
      offset={[0, kit.unit(kit.tokens.inset / 4)]}
    >
      {children}
    </Floating>
  );
}

/**
 * A control ref that keeps the handle in `own` for the component and passes
 * it on to the application's `ref`.
 */
export function useControlRef(
  own: RefObject<GuiControlHandle | null>,
  ref: Ref<GuiControlHandle> | undefined,
) {
  return useCallback(
    (handle: GuiControlHandle | null) => {
      own.current = handle;
      if (typeof ref === "function") ref(handle);
      else if (ref) ref.current = handle;
    },
    [own, ref],
  );
}
