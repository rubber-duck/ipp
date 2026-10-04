/**
 * A multi-select: a dropdown whose options toggle. The trigger summarises
 * the selection, the selected labels joined in option order while they fit
 * its width and otherwise their count, such as 3 selected; selected options
 * carry the check mark at the start of their row.
 *
 * The trigger opens and closes the list as a dropdown's does. Picking an
 * option, with a press or with Enter or Space on the active option, toggles
 * it and keeps the list open, reporting the new selection through
 * `onChange` each time; Escape, Tab or a press outside closes the list.
 *
 * The application owns the options and the selection (`value` or
 * `defaultValue`, keys in any order). The summary fits the trigger's
 * explicit `layout.width`, or its default width.
 */
import { useRef, useState } from "react";
import type { GuiKitLayout } from "./layout.js";
import { useGuiKit } from "./kit.js";
import { OptionList, type SelectOption } from "./option-list.js";
import { useOverlayOpen, type OverlayOpenProps } from "./overlay.js";
import { SelectList, SelectTrigger, selectTextRoom } from "./select-trigger.js";
import { textWidth } from "./text.js";

export interface MultiSelectProps extends OverlayOpenProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  /** Symbolic id of the trigger; the list and its rows extend it. */
  readonly id: string;
  readonly options: readonly SelectOption[];
  /** Keys of the selected options, controlled; pair it with `onChange`. */
  readonly value?: readonly string[];
  /** The initially selected keys while `value` is omitted. */
  readonly defaultValue?: readonly string[];
  /** An option was toggled: the selected keys in option order. */
  readonly onChange?: (value: readonly string[]) => void;
  /** Shown in the neutral tone while nothing is selected. */
  readonly placeholder?: string;
  /** The field's semantic name, such as Channels. */
  readonly label?: string;
  readonly disabled?: boolean;
  /** Rows the list shows before it scrolls; six by default. */
  readonly maxRows?: number;
  /** Layout of the trigger, such as its `width`; 240 at the `em` by default. */
  readonly layout?: GuiKitLayout;
}

/**
 * The summary of the selected `labels` in `room` at body size `size`: the
 * labels joined while they fit, otherwise their count.
 */
export function selectionSummary(
  labels: readonly string[],
  room: number,
  size: number,
): string {
  const joined = labels.join(", ");
  return textWidth(joined, size) <= room ? joined : `${labels.length} selected`;
}

export function MultiSelect({
  id,
  layer = 0,
  options,
  value,
  defaultValue = [],
  onChange,
  placeholder = "Select",
  label,
  disabled = false,
  maxRows,
  layout,
  ...open
}: MultiSelectProps) {
  const kit = useGuiKit();
  const overlay = useOverlayOpen(open);
  const [own, setOwn] = useState(defaultValue);
  const keys = value ?? own;
  // The selection as this client last set or rendered it, ahead of its
  // renders, so a second toggle before the first one's render builds on it.
  const latest = useRef(keys);
  const rendered = useRef(keys);
  if (rendered.current !== keys) {
    rendered.current = keys;
    latest.current = keys;
  }
  const current = new Set(keys);
  const selected = options.filter((option) => current.has(option.key));
  // Every toggle while the list is open counts: it stays open.
  const toggle = (key: string) => {
    if (!overlay.open) return;
    const before = new Set(latest.current);
    const next = options
      .filter((option) =>
        option.key === key ? !before.has(key) : before.has(option.key),
      )
      .map((option) => option.key);
    latest.current = next;
    if (value === undefined) setOwn(next);
    onChange?.(next);
  };
  const summary = selectionSummary(
    selected.map((option) => option.label),
    selectTextRoom(kit, layout),
    kit.typeSize("body"),
  );
  return (
    <SelectTrigger
      id={id}
      layer={layer}
      text={selected.length > 0 ? summary : placeholder}
      placeholder={selected.length === 0}
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
          selected={selected.map((option) => option.key)}
          mark="check"
          onPick={toggle}
          {...(maxRows === undefined ? {} : { maxRows })}
        />
      </SelectList>
    </SelectTrigger>
  );
}
