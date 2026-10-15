import { useEntityId } from "../scope/identity.js";
import { PresentationGate, Children, Entity } from "../components.js";
import { TextInput, type TextInputProps } from "../gui/controls.js";
import { Behavior, Font, Layout, Style } from "../gui/components.js";
import { useFormLayout } from "./containers/form.js";
import { useGuiKit } from "./kit.js";
import {
  LAYOUT_COLUMN,
  LAYOUT_LEAF,
  layoutFields,
  Row,
  type GuiKitLayoutProps,
} from "./layout.js";
import { TextLine } from "./text.js";
import { useTextValue } from "./values/text-value.js";

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
  readonly disabled?: boolean;
  readonly layer?: number;
}

export function TextField({
  id: providedId,
  value,
  defaultValue = "",
  onChange,
  label,
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
  ...control
}: TextFieldProps) {
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  const inForm = useFormLayout();
  const text = useTextValue(value, defaultValue, onChange, ref);
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style layer={layer} />
      <Layout
        kind={LAYOUT_COLUMN}
        {...layoutFields(
          {
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
          },
          {
            width:
              inForm || (layout?.flex ?? flex ?? 0) > 0 ? -1 : kit.unit(240),
            fit_height: true,
            gap: kit.spacing.label,
          },
        )}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Behavior enabled={!disabled} />
      <Children>
        {label !== undefined && (
          <TextLine
            id={`${id}/label`}
            text={label}
            tone={disabled ? "neutral" : "accent"}
          />
        )}
        <Row id={`${id}/row`} height={kit.unit(kit.tokens.controlHeight)}>
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
        {error !== undefined && (
          <TextLine id={`${id}/error`} text={error} tone="error" size="small" />
        )}
      </Children>
    </Entity>
  );
}
