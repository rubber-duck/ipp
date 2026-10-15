/** Native control inputs shared by the glass value sheet and its scenarios. */
export const GLASS_SELECTION_CASES = [
  { id: "checkbox-off", kind: "checkbox", checked: false, disabled: false },
  { id: "checkbox-on", kind: "checkbox", checked: true, disabled: false },
  { id: "checkbox-disabled", kind: "checkbox", checked: true, disabled: true },
  { id: "switch-off", kind: "switch", checked: false, disabled: false },
  { id: "switch-on", kind: "switch", checked: true, disabled: false },
  { id: "switch-disabled", kind: "switch", checked: true, disabled: true },
] as const;

export const GLASS_SLIDER_CASES = [
  {
    id: "horizontal",
    axis: 0,
    value: 5,
    upper: 10,
    range: false,
    disabled: false,
  },
  {
    id: "vertical",
    axis: 1,
    value: 5,
    upper: 10,
    range: false,
    disabled: false,
  },
  { id: "range", axis: 0, value: 2, upper: 8, range: true, disabled: false },
  {
    id: "vertical-range",
    axis: 1,
    value: 2,
    upper: 8,
    range: true,
    disabled: false,
  },
  {
    id: "slider-disabled",
    axis: 0,
    value: 5,
    upper: 10,
    range: false,
    disabled: true,
  },
  {
    id: "range-disabled",
    axis: 0,
    value: 2,
    upper: 8,
    range: true,
    disabled: true,
  },
] as const;

export const GLASS_SLIDER_BOUNDS = {
  min: 0,
  max: 10,
  step: 1,
  fine_step: 0.25,
} as const;

/** Flat logical rectangles; native slider travel is contained within each. */
export const GLASS_VALUE_RECTS: Readonly<
  Record<string, readonly [number, number, number, number]>
> = {
  "checkbox-off": [32, 32, 40, 40],
  "checkbox-on": [120, 32, 40, 40],
  "checkbox-disabled": [208, 32, 40, 40],
  "switch-off": [320, 36, 72, 32],
  "switch-on": [440, 36, 72, 32],
  "switch-disabled": [560, 36, 72, 32],
  horizontal: [32, 128, 240, 40],
  range: [320, 128, 240, 40],
  "slider-disabled": [32, 216, 240, 40],
  "range-disabled": [320, 216, 240, 40],
  vertical: [680, 128, 40, 240],
  "vertical-range": [776, 128, 40, 240],
};

export const GLASS_VALUES_EXTENT = [864, 416] as const;
