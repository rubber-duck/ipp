import { Button, TextInput } from "../gui/controls.js";
import type { ButtonProps, TextInputProps } from "../gui/controls.js";
import { Skin } from "../gui/theme.js";
import { useGlassSkin } from "./skin.js";

/**
 * An ordinary native button with the optional glass theme. Declare it inside
 * the caller's Entity, beside its Layout, Font and Behavior components.
 * Selection, interaction, focus and callbacks remain the Button's own.
 */
export function GlassButton(props: ButtonProps) {
  const glass = useGlassSkin();
  return (
    <>
      <Skin theme={glass.theme("button")} />
      <Button {...props} />
    </>
  );
}

/**
 * An ordinary single-line native text input with the glass theme. Numeric
 * commits, masking, selection, composition and caret use TextInput directly;
 * reskinning never creates a second value or editing state.
 */
export function GlassTextInput(props: TextInputProps) {
  const glass = useGlassSkin();
  return (
    <>
      <Skin theme={glass.theme("textInput")} />
      <TextInput {...props} />
    </>
  );
}
