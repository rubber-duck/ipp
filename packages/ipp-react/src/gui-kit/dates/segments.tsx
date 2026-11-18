import { useLayoutEffect, useRef, useState } from "react";
import { Button } from "../button.js";
import { Column, Row, type ContainerProps } from "../layout.js";
import { TextField } from "../text-field.js";
import { TextLine } from "../text.js";

export type SegmentValues = Readonly<Record<string, string>>;
export interface SegmentSpec {
  readonly key: string;
  readonly label: string;
}

interface Props extends Omit<ContainerProps, "children"> {
  readonly id: string;
  readonly label: string;
  readonly value: string | null;
  readonly required: boolean;
  readonly segments: readonly SegmentSpec[];
  readonly split: (value: string | null) => SegmentValues;
  readonly parse: (parts: SegmentValues) => string | null;
  readonly eligible: (value: string) => boolean;
  readonly paste: (text: string) => string | null;
  readonly onCommit: (value: string | null) => void;
  readonly step?: (value: string, part: string, direction: number) => string;
}

/** A draft is scoped to the owner's committed value and current segment vocabulary.
 * Reordering the same parts updates layout without replacing the editing entities. */
export function Segments({
  id,
  label,
  value,
  segments,
  required,
  split,
  parse,
  eligible,
  paste,
  onCommit,
  step,
  ...layout
}: Props) {
  const signature = segments
    .map((s) => s.key)
    .sort()
    .join("/");
  const [draft, setDraft] = useState<{
    value: string | null;
    key: string;
    parts: SegmentValues;
  } | null>(null);
  const live = useRef(draft);
  const [invalid, setInvalid] = useState(false);
  function parts() {
    const edit = live.current;
    return edit && edit.value === value && edit.key === signature
      ? edit.parts
      : split(value);
  }
  function reset() {
    live.current = null;
    setDraft(null);
    setInvalid(false);
  }
  useLayoutEffect(() => {
    reset();
  }, [value, signature]);
  function commit(input: SegmentValues) {
    const empty = segments.every(({ key }) => !input[key]);
    const next = parse(input);
    if (empty && !required) {
      reset();
      onCommit(null);
      return;
    }
    if (next === null || !eligible(next)) {
      setInvalid(true);
      return;
    }
    reset();
    onCommit(next);
  }
  function edit(key: string, text: string) {
    const whole = paste(text);
    const next = whole === null ? { ...parts(), [key]: text } : split(whole);
    live.current = { value, key: signature, parts: next };
    setDraft(live.current);
    setInvalid(false);
    return next;
  }
  const display =
    draft && draft.value === value && draft.key === signature
      ? draft.parts
      : split(value);
  return (
    <Column id={id} {...layout}>
      <TextLine text={label} />
      <Row>
        {segments.map((s) => (
          <Column key={s.key} flex={1}>
            <TextField
              id={`${id}/${s.key}`}
              label={s.label}
              value={display[s.key] ?? ""}
              onChange={(text) => edit(s.key, text)}
              onSubmit={(event) => commit(edit(s.key, event.value))}
            />
            {step && (
              <Row>
                {[-1, 1].map((d) => (
                  <Button
                    key={d}
                    id={`${id}/${s.key}/${d > 0 ? "increase" : "decrease"}`}
                    label={d < 0 ? "−" : "+"}
                    semanticLabel={`${d < 0 ? "Decrease" : "Increase"} ${s.label}`}
                    onPress={() => {
                      const at = parse(parts()) ?? value;
                      if (at === null) return;
                      const next = step(at, s.key, d);
                      if (eligible(next)) {
                        reset();
                        onCommit(next);
                      }
                    }}
                  />
                ))}
              </Row>
            )}
          </Column>
        ))}
      </Row>
      <Row>
        <Button
          id={`${id}/apply`}
          label="Apply"
          onPress={() => commit(parts())}
        />
        <Button id={`${id}/cancel`} label="Cancel" onPress={reset} />
        {!required && (
          <Button
            id={`${id}/clear`}
            label="Clear"
            onPress={() => {
              reset();
              onCommit(null);
            }}
          />
        )}
      </Row>
      {invalid && (
        <TextLine id={`${id}/error`} tone="error" text={`Invalid ${label}`} />
      )}
    </Column>
  );
}
