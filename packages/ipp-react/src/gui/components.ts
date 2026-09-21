/** GUI declarations for the optional `@ipp/react/gui` entry point.
 *
 * These components extend the existing reconciler; browser composition stays
 * in `@ipp/react/web`. Render-time work here is pure: element creation,
 * validation and snapshot description perform no transport. All runtime
 * effects happen in the commit phase through the generated GUI client.
 *
 * Identity: React keys preserve runtime nodes because the reconciler retains
 * host instances for matching keys. The commit phase maps each retained
 * instance identity to one monotonically allocated node ID which is never
 * reused, mirroring the runtime contract. Author keys are never runtime IDs.
 *
 * Handles: `nodeRef` targets resolve after acknowledgement only, and clear
 * when their node is removed or the root unmounts.
 */
import { createElement, type ReactNode } from "react";
import type {
  GuiAssetSource,
  GuiNodeContent,
  GuiNodeHandle,
  GuiNodeStyle,
} from "@ipp/client";
import type { GuiControlTheme } from "./theme.js";

export const GUI_ROOT_HOST_TYPE = "ipp-gui-root";
export const GUI_ROW_HOST_TYPE = "ipp-gui-row";
export const GUI_COLUMN_HOST_TYPE = "ipp-gui-column";
export const GUI_STACK_HOST_TYPE = "ipp-gui-stack";
export const GUI_PADDING_HOST_TYPE = "ipp-gui-padding";
export const GUI_ALIGN_HOST_TYPE = "ipp-gui-align";
export const GUI_SIZED_BOX_HOST_TYPE = "ipp-gui-sized-box";
export const GUI_SCROLL_VIEW_HOST_TYPE = "ipp-gui-scroll-view";
export const GUI_TEXT_HOST_TYPE = "ipp-gui-text";
export const GUI_DRAWING_HOST_TYPE = "ipp-gui-drawing";
export const GUI_IMAGE_HOST_TYPE = "ipp-gui-image";
export const GUI_BUTTON_HOST_TYPE = "ipp-gui-button";
export const GUI_CHECKBOX_HOST_TYPE = "ipp-gui-checkbox";
export const GUI_SLIDER_HOST_TYPE = "ipp-gui-slider";
export const GUI_TEXT_INPUT_HOST_TYPE = "ipp-gui-text-input";

export type GuiHostType =
  | typeof GUI_ROOT_HOST_TYPE
  | typeof GUI_ROW_HOST_TYPE
  | typeof GUI_COLUMN_HOST_TYPE
  | typeof GUI_STACK_HOST_TYPE
  | typeof GUI_PADDING_HOST_TYPE
  | typeof GUI_ALIGN_HOST_TYPE
  | typeof GUI_SIZED_BOX_HOST_TYPE
  | typeof GUI_SCROLL_VIEW_HOST_TYPE
  | typeof GUI_TEXT_HOST_TYPE
  | typeof GUI_DRAWING_HOST_TYPE
  | typeof GUI_IMAGE_HOST_TYPE
  | typeof GUI_BUTTON_HOST_TYPE
  | typeof GUI_CHECKBOX_HOST_TYPE
  | typeof GUI_SLIDER_HOST_TYPE
  | typeof GUI_TEXT_INPUT_HOST_TYPE;

export const guiHostTypes: ReadonlySet<string> = new Set([
  GUI_ROOT_HOST_TYPE,
  GUI_ROW_HOST_TYPE,
  GUI_COLUMN_HOST_TYPE,
  GUI_STACK_HOST_TYPE,
  GUI_PADDING_HOST_TYPE,
  GUI_ALIGN_HOST_TYPE,
  GUI_SIZED_BOX_HOST_TYPE,
  GUI_SCROLL_VIEW_HOST_TYPE,
  GUI_TEXT_HOST_TYPE,
  GUI_DRAWING_HOST_TYPE,
  GUI_IMAGE_HOST_TYPE,
  GUI_BUTTON_HOST_TYPE,
  GUI_CHECKBOX_HOST_TYPE,
  GUI_SLIDER_HOST_TYPE,
  GUI_TEXT_INPUT_HOST_TYPE,
]);

export function isGuiHostType(type: unknown): type is GuiHostType {
  return typeof type === "string" && guiHostTypes.has(type);
}

export function isGuiContainerHostType(type: unknown): boolean {
  return (
    type === GUI_ROW_HOST_TYPE ||
    type === GUI_COLUMN_HOST_TYPE ||
    type === GUI_STACK_HOST_TYPE ||
    type === GUI_PADDING_HOST_TYPE ||
    type === GUI_ALIGN_HOST_TYPE ||
    type === GUI_SIZED_BOX_HOST_TYPE ||
    type === GUI_SCROLL_VIEW_HOST_TYPE
  );
}

export function isGuiLeafHostType(type: unknown): boolean {
  return (
    type === GUI_TEXT_HOST_TYPE ||
    type === GUI_DRAWING_HOST_TYPE ||
    type === GUI_IMAGE_HOST_TYPE ||
    type === GUI_BUTTON_HOST_TYPE ||
    type === GUI_CHECKBOX_HOST_TYPE ||
    type === GUI_SLIDER_HOST_TYPE ||
    type === GUI_TEXT_INPUT_HOST_TYPE
  );
}

/** Acknowledged node identity. Populated after acknowledgement, cleared on removal. */
export type GuiNodeRef =
  | { current: GuiNodeHandle | null }
  | ((handle: GuiNodeHandle | null) => void);

/** Extension hook for later control callbacks (ipp-9nx.14).
 *
 * Listeners are stored with the declaration and dispatched along the runtime
 * logical ancestor path. They are never transported and never affect the
 * commit signature: changing a callback alone resubmits nothing. JavaScript
 * `stopPropagation` controls callbacks only; runtime defaults are declared
 * before dispatch.
 */
export type GuiActionListener = (event: GuiActionEvent) => void;

export interface GuiActionEvent {
  /** Acknowledged target node identity. */
  readonly target: number;
  /** Logical ancestor identities, root first, including the target. */
  readonly path: readonly number[];
  readonly phase: "capture" | "bubble";
  stopPropagation(): void;
  readonly propagationStopped: boolean;
}

/** Shared layout/presentation lanes. Omitted lanes keep their previous value. */
export interface GuiStyleProps {
  readonly width?: number | undefined;
  readonly height?: number | undefined;
  readonly minWidth?: number | undefined;
  readonly minHeight?: number | undefined;
  readonly maxWidth?: number | undefined;
  readonly maxHeight?: number | undefined;
  /** Content padding [top, right, bottom, left] in local metres. */
  readonly padding?: readonly [number, number, number, number] | undefined;
  /** Outer margin [top, right, bottom, left] in local metres. */
  readonly margin?: readonly [number, number, number, number] | undefined;
  readonly flex?: number | undefined;
  readonly alignX?: number | undefined;
  readonly alignY?: number | undefined;
  /** Linear RGBA in 0..1. */
  readonly color?: readonly [number, number, number, number] | undefined;
  readonly backgroundColor?:
    | readonly [number, number, number, number]
    | undefined;
  readonly opacity?: number | undefined;
  /** Font size in local metres per em. */
  readonly fontSize?: number | undefined;
  /** Bound asset reference (font, drawing or image). Binds inline through
   * the reconciler: the same source shared by two views creates no
   * duplicate asset, and swaps commit as ordinary style updates without
   * overlay teardown. */
  readonly asset?: GuiAssetSource | null | undefined;
  /** Effective interactivity; false skips hit testing and activation.
   * Defaults to true. Disabled controls never activate. */
  readonly enabled?: boolean | undefined;
}

export interface GuiNodeProps extends GuiStyleProps {
  readonly children?: ReactNode;
  /** Resolves to the acknowledged handle after acknowledgement only. */
  readonly nodeRef?: GuiNodeRef | null | undefined;
  readonly onAction?: GuiActionListener | undefined;
  readonly onActionCapture?: GuiActionListener | undefined;
  /** Runtime named-part lanes; core chooses the active interaction state. */
  readonly theme?: GuiControlTheme | undefined;
}

export interface GuiRootProps {
  readonly children?: ReactNode;
  /** Overlay mode for the GuiRoot component; defaults to automatic. */
  readonly bound?: boolean | null | undefined;
  /** Resolves to the acknowledged root-node handle after acknowledgement. */
  readonly nodeRef?: GuiNodeRef | null | undefined;
  readonly onAction?: GuiActionListener | undefined;
  readonly onActionCapture?: GuiActionListener | undefined;
}

export interface GuiTextProps extends GuiNodeProps {
  readonly text?: string | undefined;
}

export interface GuiDrawingProps extends GuiNodeProps {}

export interface GuiImageProps extends GuiNodeProps {
  /** Display size in local metres; both dimensions must be finite and positive. */
  readonly size?: readonly [number, number] | undefined;
}

/** A GuiRoot belongs to its enclosing Entity (which also owns a Surface)
 * and owns exactly one root GUI node. */
export function GuiRoot(props: GuiRootProps) {
  return createElement(GUI_ROOT_HOST_TYPE, props);
}

export function Row(props: GuiNodeProps) {
  return createElement(GUI_ROW_HOST_TYPE, props);
}

export function Column(props: GuiNodeProps) {
  return createElement(GUI_COLUMN_HOST_TYPE, props);
}

export function Stack(props: GuiNodeProps) {
  return createElement(GUI_STACK_HOST_TYPE, props);
}

export function Padding(props: GuiNodeProps) {
  return createElement(GUI_PADDING_HOST_TYPE, props);
}

export function Align(props: GuiNodeProps) {
  return createElement(GUI_ALIGN_HOST_TYPE, props);
}

export function SizedBox(props: GuiNodeProps) {
  return createElement(GUI_SIZED_BOX_HOST_TYPE, props);
}

export function ScrollView(props: GuiNodeProps) {
  return createElement(GUI_SCROLL_VIEW_HOST_TYPE, props);
}

export function Text(props: GuiTextProps) {
  return createElement(GUI_TEXT_HOST_TYPE, props);
}

export function Drawing(props: GuiDrawingProps) {
  return createElement(GUI_DRAWING_HOST_TYPE, props);
}

export function Image(props: GuiImageProps) {
  return createElement(GUI_IMAGE_HOST_TYPE, props);
}

export function validateGuiNodeRef(value: unknown): void {
  if (value == null) return;
  if (typeof value === "function") return;
  if (typeof value === "object" && "current" in value) return;
  throw new Error("GUI nodeRef must be a ref object, a callback, or null");
}

/** Complete style with lane defaults filled, plus the authoring-only
 * `enabled` lane (default true) carried for the runtime named property.
 * The shared client contract gains the lane on regeneration; until then
 * the extra property rides the runtime object without transport changes. */
export type GuiDeclarationStyle = GuiNodeStyle & {
  enabled?: boolean | undefined;
};

/** Complete style with lane defaults filled. Copies tuples defensively. */
export function guiStyleFor(props: GuiStyleProps): GuiDeclarationStyle {
  const copy4 = (
    value: readonly [number, number, number, number] | undefined,
  ): [number, number, number, number] | undefined =>
    value === undefined ? undefined : [value[0], value[1], value[2], value[3]];
  const style: GuiDeclarationStyle = {
    ...(props.width === undefined ? {} : { width: props.width }),
    ...(props.height === undefined ? {} : { height: props.height }),
    ...(props.minWidth === undefined ? {} : { minWidth: props.minWidth }),
    ...(props.minHeight === undefined ? {} : { minHeight: props.minHeight }),
    ...(props.maxWidth === undefined ? {} : { maxWidth: props.maxWidth }),
    ...(props.maxHeight === undefined ? {} : { maxHeight: props.maxHeight }),
    ...(props.padding === undefined ? {} : { padding: copy4(props.padding)! }),
    ...(props.margin === undefined ? {} : { margin: copy4(props.margin)! }),
    ...(props.flex === undefined ? {} : { flex: props.flex }),
    ...(props.alignX === undefined ? {} : { alignX: props.alignX }),
    ...(props.alignY === undefined ? {} : { alignY: props.alignY }),
    color: copy4(props.color) ?? [1, 1, 1, 1],
    ...(props.backgroundColor === undefined
      ? {}
      : { backgroundColor: copy4(props.backgroundColor)! }),
    opacity: props.opacity ?? 1,
    fontSize: props.fontSize ?? 0.1,
    enabled: props.enabled ?? true,
  };
  if (props.asset !== undefined) style.asset = props.asset;
  return style;
}

/** Slider range defaults: unit range starting at zero, continuous step. */
export const DEFAULT_SLIDER_VALUE = 0;
export const DEFAULT_SLIDER_MIN = 0;
export const DEFAULT_SLIDER_MAX = 1;
export const DEFAULT_SLIDER_STEP = 0;

function finite(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

/** Button content for one validated label. */
export function buttonContent(label: string): GuiNodeContent {
  if (typeof label !== "string")
    throw new Error("GUI Button label must be a string");
  return { kind: "button", label };
}

/** Checkbox content for one validated initial state. */
export function checkboxContent(checked: boolean | undefined): GuiNodeContent {
  if (checked !== undefined && typeof checked !== "boolean")
    throw new Error("GUI Checkbox checked must be a boolean or undefined");
  return { kind: "checkbox", checked: checked ?? false };
}

/** Slider content for one validated initial range. */
export function sliderContent(options: {
  readonly value?: number | undefined;
  readonly min?: number | undefined;
  readonly max?: number | undefined;
  readonly step?: number | undefined;
}): GuiNodeContent {
  const value = options.value ?? DEFAULT_SLIDER_VALUE;
  const min = options.min ?? DEFAULT_SLIDER_MIN;
  const max = options.max ?? DEFAULT_SLIDER_MAX;
  const step = options.step ?? DEFAULT_SLIDER_STEP;
  if (!finite(value))
    throw new Error("GUI Slider value must be a finite number or undefined");
  if (!finite(min))
    throw new Error("GUI Slider min must be a finite number or undefined");
  if (!finite(max))
    throw new Error("GUI Slider max must be a finite number or undefined");
  if (!finite(step) || step < 0)
    throw new Error(
      "GUI Slider step must be a finite number >= 0 or undefined",
    );
  if (min > max) throw new Error("GUI Slider min must not exceed max");
  return { kind: "slider", value, min, max, step };
}

/** Text-input content for one validated initial text and placeholder. */
export function textInputContent(options: {
  readonly text?: string | undefined;
  readonly placeholder?: string | undefined;
}): GuiNodeContent {
  if (options.text !== undefined && typeof options.text !== "string")
    throw new Error("GUI TextInput text must be a string or undefined");
  if (
    options.placeholder !== undefined &&
    typeof options.placeholder !== "string"
  )
    throw new Error("GUI TextInput placeholder must be a string or undefined");
  return {
    kind: "textInput",
    text: options.text ?? "",
    placeholder: options.placeholder ?? "",
  };
}

export function guiContentFor(
  type: GuiHostType,
  props: GuiTextProps &
    GuiImageProps & {
      readonly label?: string | undefined;
      readonly checked?: boolean | undefined;
      readonly value?: number | undefined;
      readonly min?: number | undefined;
      readonly max?: number | undefined;
      readonly step?: number | undefined;
      readonly text?: string | undefined;
      readonly placeholder?: string | undefined;
    },
): GuiNodeContent {
  switch (type) {
    case GUI_ROW_HOST_TYPE:
      return { kind: "container", containerKind: "row" };
    case GUI_COLUMN_HOST_TYPE:
      return { kind: "container", containerKind: "column" };
    case GUI_STACK_HOST_TYPE:
      return { kind: "container", containerKind: "stack" };
    case GUI_PADDING_HOST_TYPE:
      return { kind: "container", containerKind: "padding" };
    case GUI_ALIGN_HOST_TYPE:
      return { kind: "container", containerKind: "align" };
    case GUI_SIZED_BOX_HOST_TYPE:
      return { kind: "container", containerKind: "sizedBox" };
    case GUI_SCROLL_VIEW_HOST_TYPE:
      return { kind: "container", containerKind: "scrollView" };
    case GUI_TEXT_HOST_TYPE:
      return { kind: "text", text: props.text ?? "" };
    case GUI_DRAWING_HOST_TYPE:
      return { kind: "drawing" };
    case GUI_IMAGE_HOST_TYPE: {
      const size = props.size ?? [1, 1];
      return { kind: "image", size: [size[0]!, size[1]!] };
    }
    case GUI_BUTTON_HOST_TYPE:
      return buttonContent(props.label as string);
    case GUI_CHECKBOX_HOST_TYPE:
      return checkboxContent(props.checked);
    case GUI_SLIDER_HOST_TYPE:
      return sliderContent(props);
    case GUI_TEXT_INPUT_HOST_TYPE:
      return textInputContent(props);
    case GUI_ROOT_HOST_TYPE:
      throw new Error("GuiRoot has no node content");
  }
}
