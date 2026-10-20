/** Semantic form flow; native layout remains the only geometry owner. */
import { createContext, useContext } from "react";
import { useEntityId } from "../../scope/identity.js";
import { useGuiKit } from "../kit.js";
import { Column, type ContainerProps } from "../layout.js";
import type { GuiKitSpace } from "../spacing.js";
import { TextLine } from "../text.js";

const FormContext = createContext(false);

/** Internal field defaults; explicit control dimensions always take precedence. */
export function useFormLayout() {
  return useContext(FormContext);
}

export type FormProps = ContainerProps;

/** A content-height column with field spacing and fill-width field defaults. */
export function Form(props: FormProps) {
  const kit = useGuiKit();
  return (
    <FormContext value>
      <Column {...props} gap={props.gap ?? kit.spacing.field} />
    </FormContext>
  );
}

export interface FieldProps extends ContainerProps {
  readonly label?: string | undefined;
  readonly description?: string | undefined;
  readonly error?: string | undefined;
  readonly disabled?: boolean;
}

/** An optional caption and helper around a control; no outside margins. */
export function Field({
  id: providedId,
  label,
  description,
  error,
  disabled = false,
  children,
  ...layout
}: FieldProps) {
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  return (
    <Form id={id} {...layout} gap={layout.gap ?? kit.spacing.label}>
      {label !== undefined && (
        <TextLine
          id={`${id}/label`}
          text={label}
          tone={disabled ? "neutral" : "accent"}
        />
      )}
      {children}
      {description !== undefined && (
        <TextLine
          id={`${id}/description`}
          text={description}
          tone="neutral"
          size="small"
        />
      )}
      {error !== undefined && (
        <TextLine id={`${id}/error`} text={error} tone="error" size="small" />
      )}
    </Form>
  );
}

export interface FormSectionProps extends ContainerProps {
  readonly title?: string;
  readonly description?: string;
  /** Space between body fields, as a logical length or skin spacing role. */
  readonly fieldGap?: GuiKitSpace;
}

/** Group heading and a field-spaced body; its parent owns section separation. */
export function FormSection({
  id: providedId,
  title,
  description,
  fieldGap,
  children,
  ...layout
}: FormSectionProps) {
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  return (
    <Column id={id} {...layout} gap={layout.gap ?? kit.spacing.field}>
      {(title !== undefined || description !== undefined) && (
        <Column id={`${id}/heading`} gap={kit.spacing.label}>
          {title !== undefined && (
            <TextLine id={`${id}/title`} text={title} tone="accent" />
          )}
          {description !== undefined && (
            <TextLine
              id={`${id}/description`}
              text={description}
              tone="neutral"
              size="small"
            />
          )}
        </Column>
      )}
      <Form id={`${id}/body`} gap={fieldGap ?? kit.spacing.field}>
        {children}
      </Form>
    </Column>
  );
}
