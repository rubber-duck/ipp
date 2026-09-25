/** Skinnable control declarations for `@ipp/react/gui`.
 *
 * Public React framework for app authors: Button, Checkbox, Slider and
 * TextInput over the GuiRoot node declarations. IPP keeps
 * interaction and editing ownership: these components declare initial
 * structure only. Control values live runtime-side; ordinary commits never
 * emit revision-gated `setControlValue` writes and never replay initial
 * values over newer edits. Application callbacks observe committed effects
 * only (see `./callbacks.js`) and cannot retroactively cancel them.
 *
 * Render-time purity: element creation, validation and description perform
 * no transport. All runtime effects happen in the commit phase through the
 * generated GUI client. Callback-only changes resubmit nothing: refs and
 * callbacks are local-only and excluded from `guiRootSignature`.
 */
import { createElement } from "react";
import {
  GUI_BUTTON_HOST_TYPE,
  GUI_CHECKBOX_HOST_TYPE,
  GUI_SLIDER_HOST_TYPE,
  GUI_TEXT_INPUT_HOST_TYPE,
  buttonNode,
  checkboxNode,
  sliderNode,
  textInputNode,
  validateGuiNodeRef,
  type GuiActionListener,
  type GuiNodeRef,
  type GuiStyleProps,
} from "./components.js";
import { validateGuiTheme, type GuiControlTheme } from "./theme.js";
import type {
  GuiPressListener,
  GuiScalarCommitListener,
  GuiTextCommitListener,
  GuiToggleListener,
} from "./callbacks.js";

/** Shared lanes, refs and logical action listeners for every control. */
export interface GuiControlBaseProps extends GuiStyleProps {
  /** Resolves to the acknowledged handle after acknowledgement only. */
  readonly nodeRef?: GuiNodeRef | null | undefined;
  readonly onAction?: GuiActionListener | undefined;
  readonly onActionCapture?: GuiActionListener | undefined;
  /** Root theme this control references; core chooses the active
   * interaction state. */
  readonly theme?: GuiControlTheme | undefined;
}

export interface ButtonProps extends GuiControlBaseProps {
  /** Static label; also the semantic name. Must be a string. */
  readonly label: string;
  /** Momentary-press observer; fed by committed effects only. */
  readonly onPress?: GuiPressListener | undefined;
}

export interface CheckboxProps extends GuiControlBaseProps {
  /** Initial structure only; committed state is runtime-owned. */
  readonly checked?: boolean | undefined;
  /** Toggle observer; fed by committed effects only. */
  readonly onToggle?: GuiToggleListener | undefined;
}

export interface SliderProps extends GuiControlBaseProps {
  /** Initial structure only; committed state is runtime-owned. */
  readonly value?: number | undefined;
  /** Bounds may rerender while they still contain the committed value.
   * Incompatible domain changes reject without mutation; use an explicit
   * revision-fenced `setControlValue` correction first, or remount the keyed
   * control when replacement semantics are intended. */
  readonly min?: number | undefined;
  readonly max?: number | undefined;
  /** Step 0 declares continuous; snapping, if any, is runtime-owned. */
  readonly step?: number | undefined;
  /** Scalar-commit observer; fed by committed effects only. */
  readonly onScalarCommit?: GuiScalarCommitListener | undefined;
}

export interface TextInputProps extends GuiControlBaseProps {
  /** Initial structure only; committed text is runtime-owned. */
  readonly text?: string | undefined;
  /** Placeholder; also the semantic name while nonempty. */
  readonly placeholder?: string | undefined;
  /** Text-commit observer; fed by committed effects only. */
  readonly onTextCommit?: GuiTextCommitListener | undefined;
}

function checkListener(kind: string, name: string, value: unknown): void {
  if (value !== undefined && typeof value !== "function")
    throw new Error(`GUI ${kind} ${name} must be a function`);
}

function checkControlBase(kind: string, props: GuiControlBaseProps): void {
  validateGuiNodeRef(props.nodeRef ?? null);
  checkListener(kind, "onAction", props.onAction);
  checkListener(kind, "onActionCapture", props.onActionCapture);
  if (props.theme !== undefined) validateGuiTheme(props.theme);
}

export function validateButtonProps(props: ButtonProps): void {
  checkControlBase("Button", props);
  buttonNode(props.label);
  checkListener("Button", "onPress", props.onPress);
}

export function validateCheckboxProps(props: CheckboxProps): void {
  checkControlBase("Checkbox", props);
  checkboxNode(props.checked);
  checkListener("Checkbox", "onToggle", props.onToggle);
}

export function validateSliderProps(props: SliderProps): void {
  checkControlBase("Slider", props);
  sliderNode(props);
  checkListener("Slider", "onScalarCommit", props.onScalarCommit);
}

export function validateTextInputProps(props: TextInputProps): void {
  checkControlBase("TextInput", props);
  textInputNode(props);
  checkListener("TextInput", "onTextCommit", props.onTextCommit);
}

/** Momentary button. Label is static structure; presses are committed effects. */
export function Button(props: ButtonProps) {
  validateButtonProps(props);
  return createElement(GUI_BUTTON_HOST_TYPE, props);
}

/** Toggle checkbox. `checked` is initial structure, not a value write. */
export function Checkbox(props: CheckboxProps) {
  validateCheckboxProps(props);
  return createElement(GUI_CHECKBOX_HOST_TYPE, props);
}

/** Ranged slider. Range props are initial structure, not value writes. */
export function Slider(props: SliderProps) {
  validateSliderProps(props);
  return createElement(GUI_SLIDER_HOST_TYPE, props);
}

/** Single-line text input. Text is initial structure, not a value write. */
export function TextInput(props: TextInputProps) {
  validateTextInputProps(props);
  return createElement(GUI_TEXT_INPUT_HOST_TYPE, props);
}
