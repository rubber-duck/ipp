/** Optional GUI declarations for `@ipp/react/gui`.
 *
 * Extends the existing reconciler; browser composition stays in
 * `@ipp/react/web`. Headless use imports no DOM dependencies. GUI input,
 * observation and effect record types come from `@ipp/client`.
 */
export {
  GuiRoot,
  Row,
  Column,
  Stack,
  Padding,
  Align,
  SizedBox,
  ScrollView,
  Text,
  Drawing,
  Image,
  DEFAULT_SLIDER_VALUE,
  DEFAULT_SLIDER_MIN,
  DEFAULT_SLIDER_MAX,
  DEFAULT_SLIDER_STEP,
} from "./gui/components.js";
export type {
  GuiNodeProps,
  GuiRootProps,
  GuiTextProps,
  GuiDrawingProps,
  GuiImageProps,
  GuiStyleProps,
  GuiNodeRef,
  GuiActionEvent,
  GuiActionListener,
} from "./gui/components.js";
export { Button, Checkbox, Slider, TextInput } from "./gui/controls.js";
export type {
  GuiControlBaseProps,
  ButtonProps,
  CheckboxProps,
  SliderProps,
  TextInputProps,
} from "./gui/controls.js";
export { GUI_THEME_PARTS, defaultGuiTheme } from "./gui/theme.js";
export type {
  GuiControlTheme,
  GuiThemedPart,
  GuiThemePartStyle,
  GuiThemePartName,
  GuiThemeState,
  GuiThemeTransition,
  GuiThemeVariant,
} from "./gui/theme.js";
export type {
  GuiControlEvent,
  GuiPressEvent,
  GuiPressListener,
  GuiScalarCommitListener,
  GuiTextCommitListener,
  GuiToggleListener,
} from "./gui/callbacks.js";
export { attachCanvasGuiInput, createGuiInputSink } from "./gui/input.js";
export type {
  AttachCanvasGuiInputOptions,
  BrowserGuiInputCommand,
  GuiBlockerHit,
  GuiInputSink,
  GuiInputSinkOptions,
  GuiInputSubmitter,
  GuiViewportPoint,
} from "./gui/input.js";
export { createGuiUnhandledInputGate } from "./gui/scene-input.js";
export type { GuiUnhandledInputGate } from "./gui/scene-input.js";
export { attachTextBridge } from "./gui/text-bridge.js";
export type {
  TextBridgeCommitted,
  TextBridgeHandle,
  TextBridgeOptions,
} from "./gui/text-bridge.js";
