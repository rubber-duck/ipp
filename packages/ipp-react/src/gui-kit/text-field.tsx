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
import { useMemo, type ReactNode } from "react";
import type { GuiControlHandle } from "../gui/control-ref.js";
import { useLabelTarget } from "./containers/label.js";
import { useTextValue } from "./values/text-value.js";
import type { GuiKitSpace } from "./spacing.js";

export interface TextFieldProps
  extends Omit<
      TextInputProps,
      | "text"
      | "value"
      | "children"
      | "fields"
      | "read_only"
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
  readonly required?: boolean;
  readonly readOnly?: boolean;
  readonly invalid?: boolean;
  /** Editable box height in canvas units. */
  readonly inputHeight?: number;
  readonly trailingControl?: ReactNode;
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
  required = false,
  readOnly = false,
  invalid,
  inputHeight,
  trailingControl,
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
  const association = useLabelTarget();
  const joined = useMemo(
    () => (handle: GuiControlHandle | null) => {
      association.ref(handle);
      if (typeof ref === "function") {
        const cleanup = ref(handle);
        return () => {
          association.ref(null);
          if (typeof cleanup === "function") cleanup();
          else ref(null);
        };
      }
      if (ref) ref.current = handle;
      return () => {
        association.ref(null);
        if (ref) ref.current = null;
      };
    },
    [association.ref, ref],
  );
  const text = useTextValue(value, defaultValue, onChange, joined);
  const boxHeight = inputHeight ?? kit.unit(kit.tokens.controlHeight);
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
          required={required}
          invalid={invalid ?? !!error}
          target={association.target}
          label={label}
          description={description}
          error={error}
          disabled={disabled}
          gap={rawGap ?? gap}
        >
          <Row gap={0} id={`${id}/row`} height={boxHeight}>
            <Entity id={`${id}/input`}>
              <PresentationGate open />
              <Layout
                kind={LAYOUT_LEAF}
                height={boxHeight}
                clip={control.multiline ?? false}
                flex={1}
              />
              <Behavior
                enabled={!disabled}
                semantic_label={label ?? placeholder}
                semantic_description={description ?? ""}
                semantic_error={error ?? ""}
                semantic_required={required}
                semantic_invalid={invalid ?? !!error}
              />
              <TextInput
                {...control}
                read_only={readOnly}
                placeholder={placeholder}
                masked={masked}
                text={text.initial}
                onTextCommit={text.changed}
                ref={text.ref}
              />
            </Entity>
            {trailingControl}
          </Row>
        </Field>
      </Children>
    </Entity>
  );
}
