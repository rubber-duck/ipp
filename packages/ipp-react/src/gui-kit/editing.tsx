import { useLayoutEffect, useRef, useState } from "react";
import { useEntityId } from "../scope/identity.js";
import { Row, Column } from "./layout.js";
import { TextField, type TextFieldProps } from "./text-field.js";
import { TextLine } from "./text.js";
import { SecondaryButton } from "./secondary-button.js";
import { useGuiKit } from "./kit.js";

export interface TextareaProps
  extends Omit<TextFieldProps, "masked" | "multiline" | "purpose"> {
  /** Number of visible lines unless inputHeight is provided. */
  readonly rows?: number;
}
export function Textarea({ rows = 4, inputHeight, ...props }: TextareaProps) {
  const kit = useGuiKit();
  if (!Number.isSafeInteger(rows) || rows < 1 || rows > 256)
    throw new RangeError("Textarea rows must be 1–256");
  return (
    <TextField
      {...props}
      multiline
      inputHeight={
        inputHeight ??
        Math.max(kit.unit(kit.tokens.controlHeight), rows * kit.fontSize * 1.25)
      }
    />
  );
}

export interface PasswordInputProps
  extends Omit<TextFieldProps, "masked" | "purpose" | "multiline"> {
  readonly purpose?: "current-password" | "new-password";
  readonly revealed?: boolean;
  readonly defaultRevealed?: boolean;
  readonly onRevealChange?: (value: boolean) => void;
}
export function PasswordInput({
  id: provided,
  purpose = "current-password",
  revealed,
  defaultRevealed = false,
  onRevealChange,
  ...props
}: PasswordInputProps) {
  const id = useEntityId(provided);
  const [own, setOwn] = useState(defaultRevealed);
  const shown = revealed ?? own;
  return (
    <TextField
      {...props}
      id={id}
      masked={!shown}
      purpose={purpose === "current-password" ? 1 : 2}
      trailingControl={
        <SecondaryButton
          id={`${id}/reveal`}
          label={shown ? "Hide password" : "Show password"}
          focusable={false}
          disabled={props.disabled ?? false}
          onPress={() => {
            if (revealed === undefined) setOwn(!shown);
            onRevealChange?.(!shown);
          }}
        />
      }
    />
  );
}

export interface OtpInputProps
  extends Omit<
    TextFieldProps,
    "purpose" | "code_length" | "code_characters" | "multiline"
  > {
  readonly length: number;
  readonly characters?: string;
  readonly onComplete?: (code: string) => void;
}
export function OtpInput({
  id: provided,
  length,
  characters = "0123456789",
  value,
  defaultValue = "",
  onChange,
  onComplete,
  ...props
}: OtpInputProps) {
  if (
    !Number.isInteger(length) ||
    length < 1 ||
    length > 64 ||
    !characters.length ||
    characters.length > 128 ||
    !/^[!-~]+$/.test(characters)
  )
    throw new RangeError(
      "Expected 1–64 code slots and a nonempty printable ASCII alphabet",
    );
  const normalize = (s: string) =>
    Array.from(s)
      .filter((c) => characters.includes(c))
      .slice(0, length)
      .join("");
  const id = useEntityId(provided);
  const [observed, setObserved] = useState(() => normalize(defaultValue));
  const current = normalize(value ?? observed);
  const previous = useRef(current);
  useLayoutEffect(() => {
    previous.current = current;
  }, [current]);
  function changed(code: string) {
    const complete = code.length === length && previous.current.length < length;
    // Controlled proposals never advance the accepted completion boundary.
    previous.current = value === undefined ? code : current;
    setObserved(code);
    onChange?.(code);
    if (complete) onComplete?.(code);
  }
  return (
    <Column id={id}>
      <TextField
        {...props}
        id={`${id}/code`}
        {...(value !== undefined ? { value: current } : {})}
        defaultValue={normalize(defaultValue)}
        purpose={3}
        code_length={length}
        code_characters={characters}
        onChange={changed}
      />
      <Row id={`${id}/slots`}>
        {Array.from({ length }, (_, i) => (
          <TextLine
            key={i}
            id={`${id}/slot/${i}`}
            text={i >= current.length ? "–" : props.masked ? "•" : current[i]!}
            tone={i < current.length ? "accent" : "neutral"}
          />
        ))}
      </Row>
    </Column>
  );
}
