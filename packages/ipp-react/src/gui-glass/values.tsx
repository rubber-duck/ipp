import {
  Checkbox,
  Slider,
  type CheckboxProps,
  type SliderProps,
} from "../gui/controls.js";
import { Skin } from "../gui/theme.js";
import { useGlassSkin } from "./skin.js";

/** Native checkbox fields and callbacks, authored inside an ordinary Entity. */
export type GlassCheckboxProps = CheckboxProps;

/** A glass box with the runtime's ordinary check mark and toggle behavior. */
export function GlassCheckbox(props: GlassCheckboxProps) {
  const glass = useGlassSkin();
  return (
    <>
      <Skin theme={glass.theme("checkbox")} />
      <Checkbox {...props} />
    </>
  );
}

/** Switch appearance; checked, focus and thumb travel remain native. */
export type GlassSwitchProps = CheckboxProps;

export function GlassSwitch(props: GlassSwitchProps) {
  const glass = useGlassSkin();
  return (
    <>
      <Skin theme={glass.theme("switch")} />
      <Checkbox {...props} />
    </>
  );
}

/** Native slider fields; axis selects horizontal or vertical presentation. */
export type GlassSliderProps = SliderProps;

export function GlassSlider(props: GlassSliderProps) {
  const glass = useGlassSkin();
  return (
    <>
      <Skin theme={glass.theme("slider")} />
      <Slider {...props} />
    </>
  );
}

/** Two ordered native values with an independently focused thumb for each. */
export type GlassRangeSliderProps = Omit<SliderProps, "range">;

export function GlassRangeSlider(props: GlassRangeSliderProps) {
  return <GlassSlider {...props} range />;
}
