/**
 * A dropdown: a trigger showing the selected option's label, or a
 * placeholder, and the option list it opens below itself. The selected
 * option shows the bar and tint of a selected row.
 *
 * A press on the trigger, or Enter or Space while it holds focus, toggles
 * the list; focus stays on the trigger, since options take none. While the
 * list is open, hover and the arrows move its active option, skipping
 * disabled ones, and Enter, Space or a press picks it: the list closes and
 * `onChange` reports the key once, unless it was already selected. Escape,
 * Tab, or a press outside the list and the trigger, which is swallowed,
 * closes it without a change. The arrows start from the selected option.
 * Letter keys do nothing: the runtime returns them unhandled to the
 * adapter's client only, so the kit has no typeahead.
 *
 * The application owns the options and the selection (`value` or
 * `defaultValue`); the open state may be its own too (`open`).
 */
import { useState } from "react";
import type { ChoiceProps } from "../choice.js";
import type { GuiKitLayout } from "../layout.js";
import { OptionList, type SelectOption } from "./option-list.js";
import { useOverlayOpen, type OverlayOpenProps } from "../overlays/overlay.js";
import { SelectList, SelectTrigger } from "./select-trigger.js";

export interface DropdownProps extends ChoiceProps, OverlayOpenProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  /** Symbolic id of the trigger; the list and its rows extend it. */
  readonly id: string;
  readonly options: readonly SelectOption[];
  /** Shown in the neutral tone while nothing is selected. */
  readonly placeholder?: string;
  /** The field's semantic name, such as Skin. */
  readonly label?: string;
  readonly disabled?: boolean;
  /** Rows the list shows before it scrolls; six by default. */
  readonly maxRows?: number;
  /** Layout of the trigger, such as its `width`; 240 at the `em` by default. */
  readonly layout?: GuiKitLayout;
}

/** The selection of a single-choice selection control. */
export function useSelection({ value, defaultValue, onChange }: ChoiceProps) {
  const [own, setOwn] = useState(defaultValue);
  const current = value ?? own;
  return {
    current,
    /** Select `key`, reporting it unless it is selected already. */
    select(key: string) {
      if (key === current) return;
      if (value === undefined) setOwn(key);
      onChange?.(key);
    },
  };
}

export function Dropdown({
  id,
  layer = 0,
  options,
  placeholder = "Select",
  label,
  disabled = false,
  maxRows,
  layout,
  ...state
}: DropdownProps) {
  const overlay = useOverlayOpen(state);
  const selection = useSelection(state);
  const selected = options.find((option) => option.key === selection.current);
  // One pick for each opening: a press arriving once the list closed is
  // dropped.
  const pick = (key: string) => {
    if (!overlay.open) return;
    overlay.setOpen(false);
    selection.select(key);
  };
  return (
    <SelectTrigger
      id={id}
      layer={layer}
      text={selected?.label ?? placeholder}
      placeholder={!selected}
      open={overlay.open}
      onPress={overlay.toggle}
      disabled={disabled}
      {...(label === undefined ? {} : { label })}
      {...(layout ? { layout } : {})}
    >
      <SelectList
        id={`${id}/list`}
        open={overlay.open}
        onVisibleChange={overlay.onVisibleChange}
      >
        <OptionList
          id={`${id}/options`}
          options={options}
          selected={selected ? [selected.key] : []}
          onPick={pick}
          {...(maxRows === undefined ? {} : { maxRows })}
        />
      </SelectList>
    </SelectTrigger>
  );
}
