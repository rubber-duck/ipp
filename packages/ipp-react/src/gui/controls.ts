import { createElement } from "react";
import {
  componentContract,
  type ComponentFields,
  type ComponentProps,
} from "../components.js";
import type { GuiControlRef } from "./control-ref.js";
import type {
  GuiContextMenuListener,
  GuiFeedbackListeners,
  GuiPressListener,
  GuiSelectedChangeListener,
  GuiToggleListener,
  GuiScalarCommitListener,
  GuiColorCommitListener,
  GuiTextCommitListener,
  GuiTextDiscardListener,
  GuiTextRejectListener,
  GuiTextSubmitListener,
} from "./callbacks.js";

export type ButtonProps = ComponentProps &
  ComponentFields<"GuiButton"> &
  GuiFeedbackListeners & {
    ref?: GuiControlRef;
    onPress?: GuiPressListener;
    /** The `selected` field, first as it is and then as anyone changes it. */
    onSelectedChange?: GuiSelectedChangeListener;
    onContextMenu?: GuiContextMenuListener;
  };

export function Button(props: ButtonProps) {
  const { ref, ...fields } = props;
  return createElement(componentContract.GuiButton.host, {
    ...fields,
    controlRef: ref,
  });
}

export type CheckboxProps = ComponentProps &
  ComponentFields<"GuiCheckbox"> &
  GuiFeedbackListeners & {
    ref?: GuiControlRef;
    onToggle?: GuiToggleListener;
    onContextMenu?: GuiContextMenuListener;
  };

export function Checkbox(props: CheckboxProps) {
  const { ref, ...fields } = props;
  return createElement(componentContract.GuiCheckbox.host, {
    ...fields,
    controlRef: ref,
  });
}

/**
 * The one scalar control. `axis` presents it as a horizontal rail 0
 * (default), a vertical rail 1 or a dial 2, which paints its range over a
 * 270-degree arc in a cut housing and turns by relative vertical drags; arrow
 * keys, Home, End and the focused wheel step every presentation alike.
 *
 * With `range` it holds `value` to `upper` with a thumb for each: write both
 * props together when moving the interval, as one commit writes them in one
 * component write, and `onScalarCommit` delivers both values in one event.
 * Each thumb is a focus part, so `onFocusChange` names the focused thumb.
 */
export type SliderProps = ComponentProps &
  ComponentFields<"GuiSlider"> &
  GuiFeedbackListeners & {
    ref?: GuiControlRef;
    onScalarCommit?: GuiScalarCommitListener;
    onContextMenu?: GuiContextMenuListener;
  };

export function Slider(props: SliderProps) {
  const { ref, ...fields } = props;
  return createElement(componentContract.GuiSlider.host, {
    ...fields,
    controlRef: ref,
  });
}

/**
 * A single-line text input. With `numeric` it holds the number `value`
 * instead of `text`, shown with `precision` decimals: the text being edited
 * commits on Enter or blur, clamped to `min` and `max`, `onScalarCommit`
 * observes each committed change, including steps, and `onReject` reports
 * text that does not parse, which leaves the number. `onDiscard` reports an
 * edit that ended without being committed or refused, such as by Escape,
 * which shows the formatted number again. Up and Down step by `step`, or `fine_step` with Shift, and with
 * `step_parts` its decrement and increment parts step on press and repeat
 * while held, each disabled at its bound.
 */
export type TextInputProps = ComponentProps &
  ComponentFields<"GuiTextInput"> &
  GuiFeedbackListeners & {
    ref?: GuiControlRef;
    onTextCommit?: GuiTextCommitListener;
    onScalarCommit?: GuiScalarCommitListener;
    onSubmit?: GuiTextSubmitListener;
    onReject?: GuiTextRejectListener;
    onDiscard?: GuiTextDiscardListener;
    onContextMenu?: GuiContextMenuListener;
  };

export function TextInput(props: TextInputProps) {
  const { ref, ...fields } = props;
  return createElement(componentContract.GuiTextInput.host, {
    ...fields,
    controlRef: ref,
  });
}

/**
 * The colour control: one colour as `hue`, `saturation`, `value` and `alpha`,
 * each in 0..1, HSV on sRGB-encoded values with linear coverage alpha. It
 * paints a saturation-value field, a hue rail, with `alpha_rail` an alpha
 * rail, and a swatch, all from that one value in the same frame. The field
 * and rails are focus parts 0, 1 and 2, so `onFocusChange` names the focused
 * one; drags and keys change the channels in place and `onColorCommit`
 * delivers all four, once per frame that changed any. Hex and channel entry,
 * labels and presets are compositions around it.
 *
 * ```tsx
 * <Entity id="picker">
 *   <Color hue={0.51} saturation={0.67} value={1} alpha={1} alpha_rail
 *     onColorCommit={({ value }) => setColor(value)} />
 *   <Layout width={240} height={200} />
 * </Entity>
 * ```
 */
export type ColorProps = ComponentProps &
  ComponentFields<"GuiColor"> &
  GuiFeedbackListeners & {
    ref?: GuiControlRef;
    onColorCommit?: GuiColorCommitListener;
    onContextMenu?: GuiContextMenuListener;
  };

export function Color(props: ColorProps) {
  const { ref, ...fields } = props;
  return createElement(componentContract.GuiColor.host, {
    ...fields,
    controlRef: ref,
  });
}
