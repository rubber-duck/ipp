/** Native action controls exercised by the shared glass specimen runner. */
export const GLASS_BUTTON_CASES = [
  { name: "idle", label: "PULSE" },
  { name: "hover", label: "PULSE" },
  { name: "pressed", label: "PULSE" },
  { name: "focus", label: "PULSE" },
  { name: "selected", label: "PULSE", selected: true },
  { name: "selected-hover", label: "PULSE", selected: true },
  { name: "selected-pressed", label: "PULSE", selected: true },
  { name: "selected-focus", label: "PULSE", selected: true },
  { name: "disabled", label: "PULSE", disabled: true },
  {
    name: "selected-disabled",
    label: "PULSE",
    selected: true,
    disabled: true,
  },
] as const;

/** Numeric and masked fields retain the ordinary TextInput implementation. */
export const GLASS_TEXT_INPUT_CASES = [
  { name: "placeholder", text: "", placeholder: "Enter text" },
  { name: "text", text: "Glass" },
  { name: "hover", text: "Glass" },
  { name: "focus", text: "Glass" },
  { name: "selection", text: "Glass" },
  { name: "disabled", text: "Glass", disabled: true },
  { name: "masked", text: "Secret", masked: true },
  { name: "numeric", numeric: true, value: 12.5, precision: 1 },
  {
    name: "numeric-steps",
    numeric: true,
    value: 12.5,
    precision: 1,
    step: 0.5,
    min: 0,
    max: 20,
    step_parts: true,
  },
] as const;

export type GlassButtonCase = (typeof GLASS_BUTTON_CASES)[number]["name"];
export type GlassTextInputCase =
  (typeof GLASS_TEXT_INPUT_CASES)[number]["name"];
