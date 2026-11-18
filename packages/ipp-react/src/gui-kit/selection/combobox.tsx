import { useLayoutEffect, useRef, useState } from "react";
import { useEntityId } from "../../scope/identity.js";
import { Button } from "../button.js";
import { Column, Row, type GuiKitLayoutProps } from "../layout.js";
import { Autocomplete } from "./autocomplete.js";
import { useApplicationValue } from "./application-value.js";
import type { SelectOption } from "./option-list.js";

export interface ComboboxProps extends GuiKitLayoutProps {
  readonly id?: string;
  readonly label: string;
  readonly options: readonly SelectOption[];
  readonly value?: readonly string[];
  readonly defaultValue?: readonly string[];
  readonly onChange?: (keys: readonly string[]) => void;
  readonly onQueryChange?: (query: string) => void;
  readonly filter?: (option: SelectOption, query: string) => boolean;
  readonly multiple?: boolean;
  /** An unmatched submission becomes an application key with this exact text. */
  readonly allowCustomValue?: boolean;
  readonly disabled?: boolean;
  readonly loading?: string;
}

export function Combobox({
  id: supplied,
  label,
  options,
  value,
  defaultValue = [],
  onChange,
  onQueryChange,
  filter = (option, query) =>
    option.label.toLocaleLowerCase().includes(query.toLocaleLowerCase()),
  multiple = false,
  allowCustomValue = false,
  disabled = false,
  loading,
  ...layout
}: ComboboxProps) {
  const id = useEntityId(supplied);
  const selection = useApplicationValue(value, defaultValue, onChange);
  const [query, setQuery] = useState("");
  const available = new Map(options.map((option) => [option.key, option]));
  const displayed = (keys: readonly string[]) =>
    multiple ? [...new Set(keys)] : keys.slice(0, 1);
  const keys = displayed(selection.current);
  // Remote searches may remove every selected option from the new result
  // page. Keep the last published label only while that key is selected.
  const remembered = useRef(new Map<string, string>());
  const labelOf = (key: string) =>
    available.get(key)?.label ?? remembered.current.get(key) ?? key;
  useLayoutEffect(() => {
    remembered.current = new Map(keys.map((key) => [key, labelOf(key)]));
  });
  const latestQuery = useRef("");
  const updateQuery = (text: string) => {
    if (text === latestQuery.current) return;
    latestQuery.current = text;
    setQuery(text);
    onQueryChange?.(text);
  };
  const choose = (key: string) => {
    const option = available.get(key);
    if (disabled || option?.disabled || (!option && !allowCustomValue)) return;
    selection.change((previous) => {
      const prior = displayed(previous);
      if (prior.includes(key)) return previous;
      return multiple ? [...prior, key] : [key];
    });
    updateQuery("");
  };
  return (
    <Column {...layout} id={id}>
      <Autocomplete
        id={`${id}/field`}
        label={label}
        placeholder={label}
        disabled={disabled}
        suggestions={options.filter((option) => filter(option, query))}
        inputValue={query}
        onInputChange={updateQuery}
        onSelect={choose}
        acceptedText={() => ""}
        onCommit={(text) => {
          const matching = options.find(
            (option) => !option.disabled && option.label === text,
          );
          if (matching) choose(matching.key);
          else if (allowCustomValue && text.trim()) choose(text);
        }}
        {...(loading === undefined ? {} : { loading })}
      />
      {keys.length > 0 && (
        <Row id={`${id}/selection`}>
          {keys.map((key) => (
            <Button
              key={key}
              id={`${id}/selected/${key}`}
              label={labelOf(key)}
              semanticLabel={`Remove ${labelOf(key)}`}
              disabled={disabled}
              selected
              onPress={() =>
                selection.change((previous) =>
                  displayed(previous).filter((item) => item !== key),
                )
              }
            />
          ))}
          <Button
            id={`${id}/clear`}
            label="Clear"
            disabled={disabled}
            onPress={() =>
              selection.change((previous) => (previous.length ? [] : previous))
            }
          />
        </Row>
      )}
    </Column>
  );
}
