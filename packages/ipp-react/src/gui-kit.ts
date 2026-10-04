/**
 * `@ipp/react/gui-kit`: reusable GUI compositions built only from the
 * ordinary declarations of `@ipp/react` and `@ipp/react/gui`, drawn in the
 * built-in looks' design language from the receiving runtime's exported
 * tokens and looks. Declare one `GuiKit` in each World that uses them.
 *
 * A composition is one file in `gui-kit/`, exported here. It reads the kit
 * through `useGuiKit()`: tokens, `unit()` for its lengths, `typeSize()`,
 * `color()`, `theme()` and `reducedMotion`, the one setting every motion of
 * the kit honours, never a prop of its own. Its looks are tables in
 * `gui-kit/themes.ts`, built from tokens and built-in looks, never from
 * literal colours or copied rows; the shared parts are `layout.tsx` (rows
 * that centre their children, layout operations), `text.tsx` (text lines,
 * icon glyphs and the check mark), `icons.ts`, `separator.tsx`,
 * `secondary-button.tsx`, `arc.tsx` (rings), `row-motion.tsx` (an entity's
 * own skin row and its Host-clock motion, still under reduced motion),
 * `choice.ts` (the selection of composites over a group), `overlay.tsx`
 * (overlay bands, the open state a client keeps in step with the runtime
 * and the floating surface), `menu.tsx` (the command rows of a menu),
 * `option-list.tsx` (the option rows of the selection controls),
 * `select-trigger.tsx` (their trigger and list surface), `slider-rail.tsx`
 * (the runtime's slider and marks placed along its rail) and
 * `scalar-value.ts` (a scalar composite's values and their text), and composites
 * reuse each other's parts, such as the panel's window controls in a toast.
 * It imports only this package's sources, never examples or tests; its
 * package test extends `tests/gui-kit.test.ts` and its skin-lab specimen
 * shows its states.
 */
export {
  GuiKit,
  type GuiKitColor,
  type GuiKitContract,
  type GuiKitLook,
  type GuiKitProps,
  type GuiKitRow,
  type GuiKitTokens,
  type GuiKitTone,
  type GuiKitTypeSize,
} from "./gui-kit/kit.js";
export { GUI_KIT_ICONS, type GuiKitIcon } from "./gui-kit/icons.js";
export {
  Row,
  type GuiKitLayout,
  type RowProps,
} from "./gui-kit/layout.js";
export {
  CheckMark,
  Icon,
  TextLine,
  type CheckMarkProps,
  type IconProps,
  type TextLineProps,
} from "./gui-kit/text.js";
export {
  LabelledSeparator,
  Separator,
  type LabelledSeparatorProps,
  type SeparatorProps,
} from "./gui-kit/separator.js";
export {
  SecondaryButton,
  type SecondaryButtonProps,
} from "./gui-kit/secondary-button.js";
export {
  Panel,
  PanelFooter,
  PanelHeader,
  type PanelFooterProps,
  type PanelHeaderProps,
  type PanelProps,
} from "./gui-kit/panel.js";
export {
  WindowControl,
  WindowControls,
  type WindowControlKind,
  type WindowControlProps,
  type WindowControlsProps,
} from "./gui-kit/window-controls.js";
export { EmptyState, type EmptyStateProps } from "./gui-kit/empty-state.js";
export {
  DataGrid,
  type DataGridCell,
  type DataGridColumn,
  type DataGridProps,
  type DataGridRow,
  type DataGridSort,
} from "./gui-kit/data-grid.js";
export {
  Toast,
  ToastStack,
  type ToastAction,
  type ToastItem,
  type ToastProps,
  type ToastSeverity,
  type ToastStackProps,
} from "./gui-kit/toast.js";
export {
  InlineAlert,
  type InlineAlertAction,
  type InlineAlertProps,
  type InlineAlertSeverity,
} from "./gui-kit/inline-alert.js";
export {
  StatusBadge,
  type StatusBadgeProps,
  type StatusBadgeStatus,
} from "./gui-kit/status-badge.js";
export {
  ProgressBar,
  type ProgressBarProps,
  type ProgressBarSegment,
  type ProgressBarStatus,
  type ProgressBarTone,
} from "./gui-kit/progress-bar.js";
export { Expander, type ExpanderProps } from "./gui-kit/expander.js";
export { Spinner, type SpinnerProps } from "./gui-kit/spinner.js";
export {
  CircularProgress,
  type CircularProgressProps,
  type CircularProgressSize,
  type CircularProgressStatus,
} from "./gui-kit/circular-progress.js";
export {
  RadioGroup,
  type RadioGroupProps,
  type RadioOption,
} from "./gui-kit/radio-group.js";
export {
  SegmentedControl,
  type SegmentedControlProps,
  type SegmentedOption,
} from "./gui-kit/segmented-control.js";
export { Tabs, type TabItem, type TabsProps } from "./gui-kit/tabs.js";
export {
  Floating,
  GUI_KIT_OVERLAY_BANDS,
  useOverlayOpen,
  type FloatingProps,
  type GuiKitOverlayAlign,
  type GuiKitOverlayBand,
  type GuiKitOverlayMode,
  type GuiKitOverlaySide,
  type OverlayOpen,
  type OverlayOpenProps,
} from "./gui-kit/overlay.js";
export {
  MENU_MIN_WIDTH,
  Menu,
  type MenuItem,
  type MenuProps,
} from "./gui-kit/menu.js";
export {
  ContextMenu,
  useContextMenu,
  type ContextMenuProps,
  type ContextMenuRequest,
  type ContextMenuState,
} from "./gui-kit/context-menu.js";
export { Popover, type PopoverProps } from "./gui-kit/popover.js";
export { Tooltip, type TooltipProps } from "./gui-kit/tooltip.js";
export {
  ConfirmationDialog,
  type ConfirmationDialogProps,
} from "./gui-kit/confirmation-dialog.js";
export {
  TreeView,
  type TreeNode,
  type TreeViewHandle,
  type TreeViewProps,
} from "./gui-kit/tree-view.js";
export {
  OPTION_LIST_ROWS,
  OptionList,
  type OptionListMark,
  type OptionListProps,
  type SelectOption,
} from "./gui-kit/option-list.js";
export {
  SELECT_WIDTH,
  SelectList,
  SelectTrigger,
  type SelectListProps,
  type SelectTriggerProps,
} from "./gui-kit/select-trigger.js";
export { Dropdown, type DropdownProps } from "./gui-kit/dropdown.js";
export {
  SearchableDropdown,
  labelContains,
  type SearchableDropdownProps,
} from "./gui-kit/searchable-dropdown.js";
export {
  MultiSelect,
  selectionSummary,
  type MultiSelectProps,
} from "./gui-kit/multi-select.js";
export {
  Autocomplete,
  type AutocompleteProps,
} from "./gui-kit/autocomplete.js";
export {
  SliderScale,
  type SliderScaleMarks,
  type SliderScaleProps,
} from "./gui-kit/slider-scale.js";
export type { SliderCompositeProps } from "./gui-kit/slider-rail.js";
export {
  LabelledSlider,
  type LabelledSliderProps,
} from "./gui-kit/labelled-slider.js";
export {
  RangeSlider,
  type RangeSliderProps,
  type RangeValue,
} from "./gui-kit/range-slider.js";
export { Knob, type KnobProps } from "./gui-kit/knob.js";
export {
  NumericStepper,
  type NumericStepperProps,
} from "./gui-kit/numeric-stepper.js";
export {
  ColorPicker,
  type ColorPickerProps,
  type ColorPreset,
} from "./gui-kit/color-picker.js";
export {
  formatHex,
  hsvaToRgba,
  parseHex,
  rgbaToHsva,
  type ColorRgba,
} from "./gui-kit/color-value.js";
export type { ChoiceProps } from "./gui-kit/choice.js";
