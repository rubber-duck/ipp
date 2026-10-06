/**
 * `@ipp/react/gui-kit`: reusable GUI compositions built only from the
 * ordinary declarations of `@ipp/react` and `@ipp/react/gui`, drawn in the
 * built-in looks' design language from the receiving runtime's exported
 * tokens and looks. Declare one `GuiKit` in each World that uses them.
 *
 * A composition is one file in a family directory of `gui-kit/`
 * (`values/`, `selection/`, `overlays/`, `feedback/`, `containers/`),
 * exported here. It reads the kit through `useGuiKit()`: tokens, `unit()`
 * for its lengths, `typeSize()`, `color()`, `theme()` and `reducedMotion`,
 * the one setting every motion of the kit honours, never a prop of its own.
 * Its looks are tables in `gui-kit/themes.ts`, built from tokens and
 * built-in looks, never from literal colours or copied rows; the shared
 * parts at the `gui-kit/` root are `layout.tsx` (rows that centre their
 * children, layout operations), `text.tsx` (text lines, icon glyphs and the
 * check mark), `icons.ts`, `separator.tsx`, `secondary-button.tsx`,
 * `arc.tsx` (rings), `row-motion.tsx` (an entity's own skin row and its
 * Host-clock motion, still under reduced motion) and `choice.ts` (the
 * selection of composites over a group). Composites reuse each other's
 * parts within and across families: `overlays/overlay.tsx` (overlay bands,
 * the open state a client keeps in step with the runtime and the floating
 * surface), `overlays/menu.tsx` (the command rows of a menu),
 * `selection/option-list.tsx` (the option rows of the selection controls),
 * `selection/select-trigger.tsx` (their trigger and list surface),
 * `values/slider-rail.tsx` (the runtime's slider and marks placed along its
 * rail), `values/scalar-value.ts` (a scalar composite's values and their
 * text), and the panel's window controls in a toast. It imports only this
 * package's sources, never examples or tests; its package test extends
 * `tests/gui-kit-<family>.test.ts` (`gui-kit-core.test.ts` for the kit
 * root, shared helpers in `gui-kit-support.ts`) and its skin-lab specimen
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
} from "./gui-kit/containers/panel.js";
export {
  WindowControl,
  WindowControls,
  type WindowControlKind,
  type WindowControlProps,
  type WindowControlsProps,
} from "./gui-kit/containers/window-controls.js";
export {
  EmptyState,
  type EmptyStateProps,
} from "./gui-kit/feedback/empty-state.js";
export {
  DataGrid,
  type DataGridCell,
  type DataGridColumn,
  type DataGridProps,
  type DataGridRow,
  type DataGridSort,
} from "./gui-kit/containers/data-grid.js";
export {
  Toast,
  ToastStack,
  type ToastAction,
  type ToastItem,
  type ToastProps,
  type ToastSeverity,
  type ToastStackProps,
} from "./gui-kit/overlays/toast.js";
export {
  InlineAlert,
  type InlineAlertAction,
  type InlineAlertProps,
  type InlineAlertSeverity,
} from "./gui-kit/feedback/inline-alert.js";
export {
  StatusBadge,
  type StatusBadgeProps,
  type StatusBadgeStatus,
} from "./gui-kit/feedback/status-badge.js";
export {
  ProgressBar,
  type ProgressBarProps,
  type ProgressBarSegment,
  type ProgressBarStatus,
  type ProgressBarTone,
} from "./gui-kit/feedback/progress-bar.js";
export { Expander, type ExpanderProps } from "./gui-kit/containers/expander.js";
export { Spinner, type SpinnerProps } from "./gui-kit/feedback/spinner.js";
export {
  CircularProgress,
  type CircularProgressProps,
  type CircularProgressSize,
  type CircularProgressStatus,
} from "./gui-kit/feedback/circular-progress.js";
export {
  RadioGroup,
  type RadioGroupProps,
  type RadioOption,
} from "./gui-kit/selection/radio-group.js";
export {
  SegmentedControl,
  type SegmentedControlProps,
  type SegmentedOption,
} from "./gui-kit/selection/segmented-control.js";
export {
  Tabs,
  type TabItem,
  type TabsProps,
} from "./gui-kit/selection/tabs.js";
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
} from "./gui-kit/overlays/overlay.js";
export {
  MENU_MIN_WIDTH,
  Menu,
  type MenuItem,
  type MenuProps,
} from "./gui-kit/overlays/menu.js";
export {
  ContextMenu,
  useContextMenu,
  type ContextMenuProps,
  type ContextMenuRequest,
  type ContextMenuState,
} from "./gui-kit/overlays/context-menu.js";
export { Popover, type PopoverProps } from "./gui-kit/overlays/popover.js";
export { Tooltip, type TooltipProps } from "./gui-kit/overlays/tooltip.js";
export {
  ConfirmationDialog,
  type ConfirmationDialogProps,
} from "./gui-kit/overlays/confirmation-dialog.js";
export {
  TreeView,
  type TreeNode,
  type TreeViewHandle,
  type TreeViewProps,
} from "./gui-kit/selection/tree-view.js";
export {
  OPTION_LIST_ROWS,
  OptionList,
  type OptionListMark,
  type OptionListProps,
  type SelectOption,
} from "./gui-kit/selection/option-list.js";
export {
  SELECT_WIDTH,
  SelectList,
  SelectTrigger,
  type SelectListProps,
  type SelectTriggerProps,
} from "./gui-kit/selection/select-trigger.js";
export { Dropdown, type DropdownProps } from "./gui-kit/selection/dropdown.js";
export {
  SearchableDropdown,
  labelContains,
  type SearchableDropdownProps,
} from "./gui-kit/selection/searchable-dropdown.js";
export {
  MultiSelect,
  selectionSummary,
  type MultiSelectProps,
} from "./gui-kit/selection/multi-select.js";
export {
  Autocomplete,
  type AutocompleteProps,
} from "./gui-kit/selection/autocomplete.js";
export {
  SliderScale,
  type SliderScaleMarks,
  type SliderScaleProps,
} from "./gui-kit/values/slider-scale.js";
export type { SliderCompositeProps } from "./gui-kit/values/slider-rail.js";
export {
  LabelledSlider,
  type LabelledSliderProps,
} from "./gui-kit/values/labelled-slider.js";
export {
  RangeSlider,
  type RangeSliderProps,
  type RangeValue,
} from "./gui-kit/values/range-slider.js";
export { Knob, type KnobProps } from "./gui-kit/values/knob.js";
export {
  NumericStepper,
  type NumericStepperProps,
} from "./gui-kit/values/numeric-stepper.js";
export {
  ColorPicker,
  type ColorPickerProps,
  type ColorPreset,
} from "./gui-kit/values/color-picker.js";
export {
  formatHex,
  hsvaToRgba,
  parseHex,
  rgbaToHsva,
  type ColorRgba,
} from "./gui-kit/values/color-value.js";
export type { ChoiceProps } from "./gui-kit/choice.js";
