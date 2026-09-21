/** Optional GUI declarations for `@ipp/react/gui`.
 *
 * Extends the existing reconciler; browser composition stays in
 * `@ipp/react/web`. Headless use imports no DOM dependencies.
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
} from "./gui/components.js";
export type {
  GuiDeclarationStyle,
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
export {
  dispatchGuiAction,
  resolveGuiActionPath,
} from "./gui/description.js";
export type { GuiActionListeners } from "./gui/description.js";
export {
  Button,
  Checkbox,
  Slider,
  TextInput,
  buttonContent,
  checkboxContent,
  controlDeclarationSignature,
  describeButton,
  describeCheckbox,
  describeSlider,
  describeTextInput,
  isGuiControlHostType,
  sliderContent,
  textInputContent,
  GUI_BUTTON_HOST_TYPE,
  GUI_CHECKBOX_HOST_TYPE,
  GUI_SLIDER_HOST_TYPE,
  GUI_TEXT_INPUT_HOST_TYPE,
  guiControlHostTypes,
  DEFAULT_SLIDER_VALUE,
  DEFAULT_SLIDER_MIN,
  DEFAULT_SLIDER_MAX,
  DEFAULT_SLIDER_STEP,
  validateButtonProps,
  validateCheckboxProps,
  validateSliderProps,
  validateTextInputProps,
} from "./gui/controls.js";
export type {
  GuiControlHostType,
  GuiControlBaseProps,
  GuiControlDeclaration,
  ButtonProps,
  CheckboxProps,
  SliderProps,
  TextInputProps,
} from "./gui/controls.js";
export {
  GUI_THEME_PARTS,
  defaultGuiTheme,
  guiThemeProperties,
  validateGuiTheme,
  validateThemeLaneStyle,
} from "./gui/theme.js";
export type {
  GuiControlTheme,
  GuiThemedPart,
  GuiThemeLaneStyle,
  GuiThemePartName,
  GuiThemeState,
  GuiThemeTransition,
  GuiThemeVariant,
} from "./gui/theme.js";
export {
  actionsForControlKind,
  controlKindForContent,
  dispatchControlEffects,
  dispatchGuiObservations,
  GuiEffectSubscriptions,
  isCommittedEffect,
  nameForContent,
  refreshesSemantics,
} from "./gui/callbacks.js";
export type {
  GuiButtonPressedEffect,
  GuiCallbackResolution,
  GuiCallbackSummary,
  GuiCancelObservation,
  GuiCancelReason,
  GuiCommittedEffect,
  GuiConflictObservation,
  GuiConflictReason,
  GuiControlAction,
  GuiControlCommittedEffect,
  GuiControlEvent,
  GuiControlKind,
  GuiControlListenerRecord,
  GuiObservationBatch,
  GuiObservationSink,
  GuiObservationSummary,
  GuiObservationTarget,
  GuiPressEvent,
  GuiPressListener,
  GuiScalarCommitListener,
  GuiTextCommitListener,
  GuiToggleListener,
  GuiUnhandledObservation,
  GuiUnhandledReason,
} from "./gui/callbacks.js";
export {
  attachCanvasGuiInput,
  canvasRelativePoint,
  canvasViewportPoint,
  createGuiInputSink,
  domMouseButtonToGuiButton,
  keyboardKeyToGuiKey,
  toGuiInputCommand,
  wheelDeltaToLogical,
} from "./gui/input.js";
export type {
  AttachCanvasGuiInputOptions,
  BrowserGuiInputCommand,
  GuiBlockerHit,
  GuiInputSink,
  GuiInputSinkOptions,
  GuiInputSubmitter,
  GuiKey,
  GuiLogicalPoint,
  GuiPointerButton,
  GuiViewportPoint,
} from "./gui/input.js";
export { createGuiUnhandledInputGate } from "./gui/scene-input.js";
export type { GuiUnhandledInputGate } from "./gui/scene-input.js";
export {
  attachImeBridge,
  createImeBridge,
  mapCompositionEnd,
  mapCompositionUpdate,
  shouldSkipBeforeInput,
  utf8ByteLength,
} from "./gui/ime.js";
export type {
  AttachImeBridgeOptions,
  ImeBridge,
  ImeBridgeOptions,
  ImeEventTarget,
} from "./gui/ime.js";
export {
  CLIPBOARD_TEXT_MAX_BYTES,
  copyFocusedTextToClipboard,
  pasteClipboardToFocusedInput,
  queryClipboardPermission,
  readClipboardText,
  resolveClipboardReader,
  resolveClipboardWriter,
  writeClipboardText,
} from "./gui/clipboard.js";
export type {
  ClipboardPermissionState,
  ClipboardReadOptions,
  ClipboardReadResult,
  ClipboardSinkOptions,
  ClipboardTextReader,
  ClipboardTextWriter,
  ClipboardWriteOptions,
  ClipboardWriteResult,
} from "./gui/clipboard.js";
export {
  dismissSoftKeyboard,
  isTrustedSoftKeyboardTrigger,
  requestSoftKeyboard,
  resolveVirtualKeyboard,
  shouldHideSoftKeyboardOnBlur,
  shouldShowSoftKeyboard,
} from "./gui/soft-keyboard.js";
export {
  attachTextBridge,
  utf16UnitsToUtf8Bytes,
  utf8BytesToUtf16Units,
} from "./gui/text-bridge.js";
export type {
  TextBridgeCommitted,
  TextBridgeHandle,
  TextBridgeOptions,
} from "./gui/text-bridge.js";
export type {
  SoftKeyboardHideOutcome,
  SoftKeyboardHookOptions,
  SoftKeyboardPolicyOptions,
  SoftKeyboardRequest,
  SoftKeyboardShowOutcome,
  SoftKeyboardTrigger,
  VirtualKeyboardLike,
} from "./gui/soft-keyboard.js";
