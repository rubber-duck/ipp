import { useEntityId } from "../../scope/identity.js";
import { Button, type ButtonProps } from "../button.js";
import { Toolbar, type ToolbarProps } from "../layout.js";
import { useApplicationValue } from "./application-value.js";
import type { SelectOption } from "./option-list.js";

export interface ToggleProps
  extends Omit<ButtonProps, "selected" | "onPress" | "onSelectedChange"> {
  readonly pressed?: boolean;
  readonly defaultPressed?: boolean;
  readonly onPressedChange?: (pressed: boolean) => void;
}

/** Every routed button press updates the application intent once. */
export function Toggle({
  pressed,
  defaultPressed = false,
  onPressedChange,
  ...props
}: ToggleProps) {
  const state = useApplicationValue(pressed, defaultPressed, onPressedChange);
  return (
    <Button
      {...props}
      selected={state.current}
      onPress={() => state.change((previous) => !previous)}
    />
  );
}

export interface ToggleGroupProps extends Omit<ToolbarProps, "children"> {
  readonly options: readonly SelectOption[];
  readonly multiple?: boolean;
  readonly allowEmpty?: boolean;
  readonly value?: readonly string[];
  readonly defaultValue?: readonly string[];
  readonly onChange?: (value: readonly string[]) => void;
}

function visibleSelection(
  keys: readonly string[],
  options: readonly SelectOption[],
  multiple: boolean,
) {
  const chosen = new Set(keys);
  const ordered = options
    .filter((option) => chosen.has(option.key))
    .map((option) => option.key);
  return multiple ? ordered : ordered.slice(0, 1);
}

export function ToggleGroup({
  id: providedId,
  options,
  multiple = false,
  allowEmpty = true,
  value,
  defaultValue = [],
  onChange,
  ...layout
}: ToggleGroupProps) {
  const id = useEntityId(providedId);
  const state = useApplicationValue(value, defaultValue, onChange);
  const selected = new Set(visibleSelection(state.current, options, multiple));
  function toggle(key: string) {
    state.change((previous) => {
      const keys = visibleSelection(previous, options, multiple);
      if (keys.includes(key))
        return keys.length === 1 && !allowEmpty
          ? previous
          : keys.filter((item) => item !== key);
      return visibleSelection(
        multiple ? [...keys, key] : [key],
        options,
        multiple,
      );
    });
  }
  return (
    <Toolbar {...layout} id={id}>
      {options.map((option) => (
        <Button
          key={option.key}
          id={`${id}/${option.key}`}
          label={option.label}
          disabled={option.disabled ?? false}
          selected={selected.has(option.key)}
          onPress={() => toggle(option.key)}
        />
      ))}
    </Toolbar>
  );
}
