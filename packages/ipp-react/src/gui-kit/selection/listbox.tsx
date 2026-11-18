import { useApplicationValue } from "./application-value.js";
import {
  OptionList,
  type OptionListProps,
  type SelectOption,
} from "./option-list.js";

export interface ListboxProps
  extends Omit<OptionListProps, "selected" | "onPick" | "mark" | "focusMode"> {
  /** Selected keys in option order. Single selection admits the first surviving key. */
  readonly value?: readonly string[];
  readonly defaultValue?: readonly string[];
  readonly multiple?: boolean;
  readonly onChange?: (keys: readonly string[]) => void;
}

export function selectedOptions(
  options: readonly SelectOption[],
  keys: readonly string[],
  multiple: boolean,
) {
  const membership = new Set(keys);
  const surviving = options
    .filter((option) => membership.has(option.key))
    .map((option) => option.key);
  return multiple ? surviving : surviving.slice(0, 1);
}

/** A materialized collection. Focus moves without committing a choice. */
export function Listbox({
  value,
  defaultValue = [],
  onChange,
  multiple = false,
  options,
  ...props
}: ListboxProps) {
  const selection = useApplicationValue(value, defaultValue, onChange);
  const selected = selectedOptions(options, selection.current, multiple);
  const pick = (key: string) => {
    if (!options.some((option) => option.key === key && !option.disabled))
      return;
    selection.change((previous) => {
      const displayed = selectedOptions(options, previous, multiple);
      if (!multiple) return displayed[0] === key ? previous : [key];
      return selectedOptions(
        options,
        displayed.includes(key)
          ? displayed.filter((item) => item !== key)
          : [...displayed, key],
        true,
      );
    });
  };
  return (
    <OptionList
      {...props}
      focusMode="rows"
      options={options}
      selected={selected}
      mark={multiple ? "check" : "selected"}
      onPick={pick}
    />
  );
}
