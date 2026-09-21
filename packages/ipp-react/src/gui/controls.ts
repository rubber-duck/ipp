/** Skinnable control declarations for `@ipp/react/gui` (ipp-9nx.14).
 *
 * Public React framework for app authors: Button, Checkbox, Slider and
 * TextInput over the frozen .10 declaration contracts. IPP keeps
 * interaction and editing ownership: these components declare initial
 * structure only. Control values live runtime-side; ordinary commits never
 * emit revision-gated `setControlValue` writes and never replay initial
 * values over newer edits. Application callbacks observe committed effects
 * only (see `./callbacks.js`) and cannot retroactively cancel them.
 *
 * Render-time purity: element creation, validation and description perform
 * no transport. All runtime effects happen in the commit phase through the
 * generated GUI client. Callback-only changes resubmit nothing: refs and
 * callbacks are local-only and excluded from the declaration signature,
 * mirroring the frozen `guiRootSignature` rule.
 *
 */
import { createElement } from "react";
import type { GuiNodeContent, GuiNodeStyle } from "@ipp/client";
import {
  GUI_BUTTON_HOST_TYPE,
  GUI_CHECKBOX_HOST_TYPE,
  GUI_SLIDER_HOST_TYPE,
  GUI_TEXT_INPUT_HOST_TYPE,
  buttonContent,
  checkboxContent,
  guiStyleFor,
  sliderContent,
  textInputContent,
  validateGuiNodeRef,
  type GuiActionListener,
  type GuiNodeRef,
  type GuiStyleProps,
} from "./components.js";
export {
  DEFAULT_SLIDER_MAX,
  DEFAULT_SLIDER_MIN,
  DEFAULT_SLIDER_STEP,
  DEFAULT_SLIDER_VALUE,
  GUI_BUTTON_HOST_TYPE,
  GUI_CHECKBOX_HOST_TYPE,
  GUI_SLIDER_HOST_TYPE,
  GUI_TEXT_INPUT_HOST_TYPE,
  buttonContent,
  checkboxContent,
  sliderContent,
  textInputContent,
} from "./components.js";
import { guiContentSignature, normalizeGuiStyle } from "./description.js";
import {
  guiStyleWithTheme,
  validateGuiTheme,
  type GuiControlTheme,
} from "./theme.js";
import type {
  GuiPressListener,
  GuiScalarCommitListener,
  GuiTextCommitListener,
  GuiToggleListener,
} from "./callbacks.js";

export type GuiControlHostType =
  | typeof GUI_BUTTON_HOST_TYPE
  | typeof GUI_CHECKBOX_HOST_TYPE
  | typeof GUI_SLIDER_HOST_TYPE
  | typeof GUI_TEXT_INPUT_HOST_TYPE;

export const guiControlHostTypes: ReadonlySet<string> = new Set([
  GUI_BUTTON_HOST_TYPE,
  GUI_CHECKBOX_HOST_TYPE,
  GUI_SLIDER_HOST_TYPE,
  GUI_TEXT_INPUT_HOST_TYPE,
]);

export function isGuiControlHostType(
  type: unknown,
): type is GuiControlHostType {
  return typeof type === "string" && guiControlHostTypes.has(type);
}

/** Shared lanes, refs and logical action listeners for every control. */
export interface GuiControlBaseProps extends GuiStyleProps {
  /** Resolves to the acknowledged handle after acknowledgement only. */
  readonly nodeRef?: GuiNodeRef | null | undefined;
  readonly onAction?: GuiActionListener | undefined;
  readonly onActionCapture?: GuiActionListener | undefined;
  /** Runtime named-part lanes; core chooses the active interaction state. */
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
  buttonContent(props.label);
  checkListener("Button", "onPress", props.onPress);
}

export function validateCheckboxProps(props: CheckboxProps): void {
  checkControlBase("Checkbox", props);
  checkboxContent(props.checked);
  checkListener("Checkbox", "onToggle", props.onToggle);
}

export function validateSliderProps(props: SliderProps): void {
  checkControlBase("Slider", props);
  sliderContent(props);
  checkListener("Slider", "onScalarCommit", props.onScalarCommit);
}

export function validateTextInputProps(props: TextInputProps): void {
  checkControlBase("TextInput", props);
  textInputContent(props);
  checkListener("TextInput", "onTextCommit", props.onTextCommit);
}

/** Pure declaration record consumed by the reconciler description pass. */
export interface GuiControlDeclaration {
  readonly hostType: GuiControlHostType;
  readonly content: GuiNodeContent;
  /** Complete style with lane defaults filled. */
  readonly style: GuiNodeStyle;
  readonly nodeRef: GuiNodeRef | null;
  readonly onAction: GuiActionListener | undefined;
  readonly onActionCapture: GuiActionListener | undefined;
  readonly onPress: GuiPressListener | undefined;
  readonly onToggle: GuiToggleListener | undefined;
  readonly onScalarCommit: GuiScalarCommitListener | undefined;
  readonly onTextCommit: GuiTextCommitListener | undefined;
  readonly theme?: GuiControlTheme | undefined;
}

function baseOf(
  props: GuiControlBaseProps,
): Pick<
  GuiControlDeclaration,
  "style" | "nodeRef" | "onAction" | "onActionCapture" | "theme"
> {
  return {
    style: guiStyleWithTheme(guiStyleFor(props), props.theme),
    nodeRef: props.nodeRef ?? null,
    onAction: props.onAction,
    onActionCapture: props.onActionCapture,
    ...(props.theme === undefined ? {} : { theme: props.theme }),
  };
}

const noControlCallback = {
  onPress: undefined,
  onToggle: undefined,
  onScalarCommit: undefined,
  onTextCommit: undefined,
} as const;

/** Describe one Button without transport. */
export function describeButton(props: ButtonProps): GuiControlDeclaration {
  validateButtonProps(props);
  return {
    hostType: GUI_BUTTON_HOST_TYPE,
    content: buttonContent(props.label),
    ...baseOf(props),
    ...noControlCallback,
    onPress: props.onPress,
  };
}

/** Describe one Checkbox without transport. */
export function describeCheckbox(props: CheckboxProps): GuiControlDeclaration {
  validateCheckboxProps(props);
  return {
    hostType: GUI_CHECKBOX_HOST_TYPE,
    content: checkboxContent(props.checked),
    ...baseOf(props),
    ...noControlCallback,
    onToggle: props.onToggle,
  };
}

/** Describe one Slider without transport. */
export function describeSlider(props: SliderProps): GuiControlDeclaration {
  validateSliderProps(props);
  return {
    hostType: GUI_SLIDER_HOST_TYPE,
    content: sliderContent(props),
    ...baseOf(props),
    ...noControlCallback,
    onScalarCommit: props.onScalarCommit,
  };
}

/** Describe one TextInput without transport. */
export function describeTextInput(
  props: TextInputProps,
): GuiControlDeclaration {
  validateTextInputProps(props);
  return {
    hostType: GUI_TEXT_INPUT_HOST_TYPE,
    content: textInputContent(props),
    ...baseOf(props),
    ...noControlCallback,
    onTextCommit: props.onTextCommit,
  };
}

/** Commit signature for one control declaration.
 *
 * Refs and every callback are local-only and excluded: changing a callback
 * alone resubmits nothing, mirroring the frozen `guiRootSignature` rule.
 */
export function controlDeclarationSignature(
  decl: GuiControlDeclaration,
): string {
  return JSON.stringify([
    decl.hostType,
    guiContentSignature(decl.content),
    normalizeGuiStyle(decl.style),
    decl.theme ?? null,
  ]);
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
