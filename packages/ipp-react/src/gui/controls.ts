import { createElement } from "react";
import {
  componentContract,
  type ComponentFields,
  type ComponentProps,
} from "../components.js";
import type { GuiControlRef } from "./control-ref.js";
import type {
  GuiPressListener,
  GuiToggleListener,
  GuiScalarCommitListener,
  GuiTextCommitListener,
  GuiTextSubmitListener,
} from "./callbacks.js";

export type ButtonProps = ComponentProps &
  ComponentFields<"GuiButton"> & {
    ref?: GuiControlRef;
    onPress?: GuiPressListener;
  };

export function Button(props: ButtonProps) {
  const { ref, ...fields } = props;
  return createElement(componentContract.GuiButton.host, {
    ...fields,
    controlRef: ref,
  });
}

export type CheckboxProps = ComponentProps &
  ComponentFields<"GuiCheckbox"> & {
    ref?: GuiControlRef;
    onToggle?: GuiToggleListener;
  };

export function Checkbox(props: CheckboxProps) {
  const { ref, ...fields } = props;
  return createElement(componentContract.GuiCheckbox.host, {
    ...fields,
    controlRef: ref,
  });
}

export type SliderProps = ComponentProps &
  ComponentFields<"GuiSlider"> & {
    ref?: GuiControlRef;
    onScalarCommit?: GuiScalarCommitListener;
  };

export function Slider(props: SliderProps) {
  const { ref, ...fields } = props;
  return createElement(componentContract.GuiSlider.host, {
    ...fields,
    controlRef: ref,
  });
}

export type TextInputProps = ComponentProps &
  ComponentFields<"GuiTextInput"> & {
    ref?: GuiControlRef;
    onTextCommit?: GuiTextCommitListener;
    onSubmit?: GuiTextSubmitListener;
  };

export function TextInput(props: TextInputProps) {
  const { ref, ...fields } = props;
  return createElement(componentContract.GuiTextInput.host, {
    ...fields,
    controlRef: ref,
  });
}
