import { useEntityId } from "../scope/identity.js";
import { PresentationGate, Children, Entity } from "../components.js";
import { TextInput, type TextInputProps } from "../gui/controls.js";
import { Behavior, Font, Layout, Style } from "../gui/components.js";
import { Field, useFormLayout } from "./containers/form.js";
import { useGuiKit } from "./kit.js";
import {
  LAYOUT_COLUMN,
  LAYOUT_LEAF,
  useLayoutFields,
  Row,
  type GuiKitLayoutProps,
} from "./layout.js";
import { useTextValue } from "./values/text-value.js";
import type { GuiKitSpace } from "./spacing.js";

export interface TextFieldProps
  extends Omit<
      TextInputProps,
      | "text"
      | "value"
      | "children"
      | "fields"
      | "numeric"
      | "onTextCommit"
      | "min"
      | "max"
      | "step"
      | "fine_step"
      | "precision"
      | "step_parts"
    >,
    GuiKitLayoutProps {
  readonly id?: string;
  /** Application text synchronized with the runtime, rather than input veto. */
  readonly value?: string;
  readonly defaultValue?: string;
  readonly onChange?: (value: string) => void;
  readonly label?: string;
  readonly error?: string;
  readonly description?: string;
  /** Spacing between the label, input, description and error. Raw layout.gap wins. */
  readonly gap?: GuiKitSpace;
  readonly disabled?: boolean;
  readonly layer?: number;
}

export function TextField({
  id: providedId,
  value,
  defaultValue = "",
  onChange,
  label,
  description,
  error,
  disabled = false,
  layer = 0,
  ref,
  placeholder = "",
  masked = false,
  layout,
  width,
  height,
  minWidth,
  minHeight,
  maxWidth,
  maxHeight,
  flex,
  fitWidth,
  fitHeight,
  alignX,
  alignY,
  padding,
  margin,
  clip,
  gap,
  ...control
}: TextFieldProps) {
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  const inForm = useFormLayout();
  const text = useTextValue(value, defaultValue, onChange, ref);
  const { gap: rawGap, ...rootLayout } = layout ?? {};
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style layer={layer} />
      <Layout
        kind={LAYOUT_COLUMN}
        {...useLayoutFields(
          {
            layout: rootLayout,
            width,
            height,
            minWidth,
            minHeight,
            maxWidth,
            maxHeight,
            flex,
            fitWidth,
            fitHeight,
            alignX,
            alignY,
            padding,
            margin,
            clip,
          },
          {
            width:
              inForm || (layout?.flex ?? flex ?? 0) > 0 ? -1 : kit.unit(240),
            fit_height: true,
          },
        )}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Behavior enabled={!disabled} />
      <Children>
        <Field
          id={`${id}/field`}
          label={label}
          description={description}
          error={error}
          disabled={disabled}
          gap={rawGap ?? gap}
        >
          <Row
            gap={0}
            id={`${id}/row`}
            height={kit.unit(kit.tokens.controlHeight)}
          >
            <Entity id={`${id}/input`}>
              <PresentationGate open />
              <Layout
                kind={LAYOUT_LEAF}
                height={kit.unit(kit.tokens.controlHeight)}
                flex={1}
              />
              <Behavior
                enabled={!disabled}
                semantic_label={label ?? placeholder}
              />
              <TextInput
                {...control}
                placeholder={placeholder}
                masked={masked}
                text={text.initial}
                onTextCommit={text.changed}
                ref={text.ref}
              />
            </Entity>
          </Row>
        </Field>
      </Children>
    </Entity>
  );
}
