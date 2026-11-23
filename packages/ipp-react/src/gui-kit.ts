/**
 * `@ipp/react/gui-kit`: reusable GUI compositions built only from the
 * ordinary declarations of `@ipp/react` and `@ipp/react/gui`, drawn with an
 * explicitly selected client skin. Declare one `GuiKit` in each World that
 * uses them.
 *
 * A composition is one file in a family directory of `gui-kit/`
 * (`values/`, `selection/`, `overlays/`, `feedback/`, `containers/`),
 * exported here. It reads the kit through `useGuiKit()`: tokens, `unit()`
 * for its lengths, `typeSize()`, `color()`, `theme()` and `reducedMotion`,
 * the one setting every motion of the kit honours, never a prop of its own.
 * Skins supply its tokens and theme tables; reusable recipe builders live in
 * `gui-kit/themes.ts`. The shared
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
  type GuiKitAppearance,
  type GuiKitControlName,
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
  ActionRow,
  type ActionRowProps,
  Toolbar,
  Column,
  OverlayStack,
  type ContainerProps,
  type GuiKitLayoutProps,
  type GuiKitFlowLayoutProps,
  type ToolbarProps,
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
  PanelBody,
  PanelFooter,
  PanelHeader,
  type PanelFooterProps,
  type PanelHeaderProps,
  type PanelProps,
  type PanelBodyProps,
} from "./gui-kit/containers/panel.js";
export {
  ScrollArea,
  type ScrollAreaProps,
} from "./gui-kit/containers/scroll-area.js";
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
  Stepper,
  type StepperProps,
  type StepperStep,
} from "./gui-kit/selection/stepper.js";
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
  linearColor,
  parseHex,
  rgbaToHsva,
  type ColorRgba,
} from "./gui-kit/values/color-value.js";
export type { ChoiceProps } from "./gui-kit/choice.js";

export type { GuiSkinDefinition, GuiSkinTheme } from "./gui/skin-types.js";
export { ControlSkin } from "./gui-kit/control-skin.js";
export { useGuiKit, type GuiKitScope } from "./gui-kit/kit.js";

export { Button, type ButtonProps } from "./gui-kit/button.js";
export {
  Checkbox,
  Switch,
  type CheckboxProps,
  type SwitchProps,
} from "./gui-kit/checkbox.js";
export { TextField, type TextFieldProps } from "./gui-kit/text-field.js";
export {
  Dialog,
  type DialogRootProps,
  type DialogTriggerProps,
  type DialogContentProps,
  type DialogHeaderProps,
  type DialogSectionProps,
  type DialogCloseProps,
} from "./gui-kit/overlays/dialog.js";

export type { PlotSkinAppearance } from "./plots/skin-types.js";

export type {
  GuiKitSpace,
  GuiKitSpacing,
  GuiKitSpacingOverrides,
} from "./gui-kit/spacing.js";
export {
  Form,
  Field,
  FormSection,
  type FormProps,
  type FieldProps,
  type FormSectionProps,
} from "./gui-kit/containers/form.js";
export {
  Toggle,
  ToggleGroup,
  type ToggleProps,
  type ToggleGroupProps,
} from "./gui-kit/selection/toggle.js";
export {
  Pagination,
  paginationWindow,
  type PaginationProps,
} from "./gui-kit/selection/pagination.js";
export {
  Calendar,
  type CalendarProps,
  type CalendarSelection,
} from "./gui-kit/dates/calendar.js";
export {
  civilDate,
  parseCivilDate,
  type CivilDate,
  type CivilRange,
  type DateBounds,
} from "./gui-kit/dates/civil-date.js";
export { Meter, type MeterProps } from "./gui-kit/feedback/meter.js";
export { DateInput, type DateInputProps } from "./gui-kit/dates/date-input.js";
export { TimeInput, type TimeInputProps } from "./gui-kit/dates/time-input.js";
export { Avatar, type AvatarProps } from "./gui-kit/feedback/avatar.js";
export {
  Timeline,
  type TimelineItem,
  type TimelineProps,
} from "./gui-kit/feedback/timeline.js";
export {
  DatePicker,
  type DatePickerProps,
} from "./gui-kit/dates/date-picker.js";
export { Rating, type RatingProps } from "./gui-kit/values/rating.js";
export {
  Resizable,
  fitPanes,
  type SplitPane,
  type PaneSizes,
  type ResizableProps,
} from "./gui-kit/containers/resizable.js";
export {
  Waterfall,
  type WaterfallSpan,
  type WaterfallProps,
} from "./gui-kit/feedback/waterfall.js";
export {
  Carousel,
  type CarouselSlide,
  type CarouselProps,
} from "./gui-kit/containers/carousel.js";
export {
  Collapsible,
  Accordion,
  type CollapsibleProps,
  type AccordionItem,
  type AccordionProps,
} from "./gui-kit/containers/disclosure.js";
export {
  Preferences,
  createGuiPreferences,
  browserMotionPreference,
  browserPreferenceStorage,
  type PreferencesProps,
  type GuiPreferencesSettings,
  type GuiPreferenceStorage,
  type GuiMotionPreference,
} from "./gui-kit/preferences.js";
export { Listbox, type ListboxProps } from "./gui-kit/selection/listbox.js";
export {
  Command,
  type CommandProps,
  type CommandItem,
} from "./gui-kit/overlays/command.js";
export { Combobox, type ComboboxProps } from "./gui-kit/selection/combobox.js";
export {
  Menubar,
  type MenubarMenu,
  type MenubarProps,
} from "./gui-kit/overlays/menubar.js";
export {
  NavigationMenu,
  type NavigationDestination,
  type NavigationSection,
  type NavigationMenuProps,
} from "./gui-kit/overlays/navigation-menu.js";
export {
  HoverCard,
  type HoverCardProps,
} from "./gui-kit/overlays/hover-card.js";
export { Drawer, type DrawerProps } from "./gui-kit/overlays/drawer.js";
export {
  ImageCropper,
  constrainCrop,
  type CropRect,
  type ImageCropperProps,
} from "./gui-kit/containers/image-cropper.js";
export {
  Label,
  useLabelTarget,
  type LabelProps,
} from "./gui-kit/containers/label.js";
export {
  Textarea,
  PasswordInput,
  OtpInput,
  type TextareaProps,
  type PasswordInputProps,
  type OtpInputProps,
} from "./gui-kit/editing.js";
export { TOKEN_DOCUMENT_CLIPBOARD_TYPE } from "./gui/platform/clipboard.js";
export { TokenInput, type TokenInputProps } from "./gui-kit/token-input.js";
export {
  normalizeTokenDocument,
  importTokenDocument,
  serializeTokenDocument,
  replaceTokenSelection,
  type TokenPart,
  type TokenDocument,
  type TokenSelection,
  type TokenClipboardData,
} from "./gui-kit/token-document.js";
export {
  FileInput,
  validateGuiFile,
  type GuiFileAdapter,
  type GuiFileDescriptor,
  type FileInputProps,
  type GuiFileRejection,
  type GuiFileRequest,
} from "./gui-kit/file-input.js";

export {
  TriggerMenu,
  matchTextTrigger,
  type TriggerMenuProps,
  type TriggerOption,
  type TriggerMatch,
} from "./gui-kit/overlays/trigger-menu.js";
