/**
 * A searchable dropdown: a dropdown whose list sits under a search field on
 * its floating surface, the text input look with a leading magnifier. The trigger keeps showing the selection while the
 * application's options are searched, so the selection stays until an
 * option is picked.
 *
 * Opening the list moves focus into the search field, the overlay's first
 * focusable control. Typing filters the options: by default those whose
 * label contains the text, ignoring case. The arrows move the active option
 * while the caret stays in the field, and Enter picks it; Enter without an
 * active option picks the first enabled match. Picking closes the list and
 * reports the key through `onChange` unless it was already selected; Escape,
 * Tab or a press outside closes it without a change. Either way focus
 * returns to the trigger and the search is cleared. Without matches the list
 * shows its empty row; `loading` adds the spinner's row, for options the
 * application is still fetching after `onQueryChange`.
 */
import { useEffect, useRef, useState, type Ref } from "react";
import { Children, Entity } from "../components.js";
import { TextInput } from "../gui/controls.js";
import { Behavior, Layout } from "../gui/components.js";
import type {
  GuiSubmitEvent,
  GuiTextCommitListener,
} from "../gui/callbacks.js";
import type { GuiControlHandle } from "../gui/control-ref.js";
import { useSelection, type DropdownProps } from "./dropdown.js";
import { useGuiKit } from "./kit.js";
import { LAYOUT_ROW, Strut } from "./layout.js";
import { OptionList, type SelectOption } from "./option-list.js";
import { useOverlayOpen } from "./overlay.js";
import { Icon } from "./text.js";
import { SelectList, SelectTrigger, useControlRef } from "./select-trigger.js";

export interface SearchableDropdownProps extends DropdownProps {
  /** Whether `option` matches the search text `query`. */
  readonly filter?: (option: SelectOption, query: string) => boolean;
  /** The search text changed, as typed or cleared on closing. */
  readonly onQueryChange?: (query: string) => void;
  /** Show the loading row with this text, such as Loading…. */
  readonly loading?: string;
  /** The search field's placeholder; Search by default. */
  readonly searchPlaceholder?: string;
  /** The empty row's text; No results by default. */
  readonly emptyLabel?: string;
  /** The search field's control handle. */
  readonly searchRef?: Ref<GuiControlHandle>;
}

/** Whether `option`'s label contains `query`, ignoring case. */
export function labelContains(option: SelectOption, query: string): boolean {
  return option.label.toLowerCase().includes(query.trim().toLowerCase());
}

/**
 * Clear the text of the field `handle`, which last reported `reported`. The
 * field lies in a closed list, where semantic actions are refused, so this
 * writes its field: from the reported text, or else from the text it holds.
 */
async function clearText(
  handle: GuiControlHandle,
  reported: string,
): Promise<void> {
  if (reported !== "" && (await handle.compareAndSet("text", reported, "")))
    return;
  const { text } = await handle.read();
  if (typeof text === "string" && text !== "")
    await handle.compareAndSet("text", text, "");
}

export function SearchableDropdown({
  id,
  layer = 0,
  options,
  placeholder = "Select",
  label,
  disabled = false,
  maxRows,
  layout,
  filter = labelContains,
  onQueryChange,
  loading,
  searchPlaceholder = "Search",
  emptyLabel = "No results",
  searchRef,
  ...state
}: SearchableDropdownProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const overlay = useOverlayOpen(state);
  const selection = useSelection(state);
  const selected = options.find((option) => option.key === selection.current);
  // The search text as the field last reported it.
  const [query, setQuery] = useState("");
  const field = useRef<GuiControlHandle | null>(null);
  const attach = useControlRef(field, searchRef);
  const open = overlay.open;

  const pick = (key: string) => {
    if (!overlay.open) return;
    overlay.setOpen(false);
    selection.select(key);
  };
  const search: GuiTextCommitListener = (event) => {
    if (event.value === query) return;
    setQuery(event.value);
    onQueryChange?.(event.value);
  };
  // Enter without an active option: the submitted text is the field's own,
  // newer than any report of it.
  const submit = (event: GuiSubmitEvent) => {
    const first = options.find(
      (option) => !option.disabled && filter(option, event.value),
    );
    if (first) pick(first.key);
  };

  // Closing clears the search, so the next opening shows every option.
  const shown = useRef(open);
  useEffect(() => {
    const closed = shown.current && !open;
    shown.current = open;
    if (closed && field.current)
      void clearText(field.current, query).catch(() => {});
  }, [open]);

  const gap = kit.unit(t.inset / 2);
  const inset = kit.unit(t.inset);
  const icon = kit.unit(t.icon);
  const height = kit.unit(t.controlHeight);
  return (
    <SelectTrigger
      id={id}
      layer={layer}
      text={selected?.label ?? placeholder}
      placeholder={!selected}
      open={open}
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
        <Entity id={`${id}/search`}>
          <Layout
            kind={LAYOUT_ROW}
            height={gap + height}
            padding_left={gap}
            padding_right={gap}
            padding_top={gap}
          />
          <Children>
            <Entity id={`${id}/search/field`}>
              <Layout
                kind={LAYOUT_ROW}
                height={height}
                flex={1}
                // The field's text starts an inset after its content box:
                // after the magnifier, which a negative margin moves back to
                // the field's own inset.
                padding_left={icon + gap}
              />
              <Behavior semantic_label={searchPlaceholder} />
              <TextInput
                placeholder={searchPlaceholder}
                onTextCommit={search}
                onSubmit={submit}
                ref={attach}
              />
              <Children>
                <Strut id={`${id}/search/field/strut`} height={height} />
                <Icon
                  id={`${id}/search/field/icon`}
                  icon="search"
                  tone="text"
                  layout={{ margin_left: inset - icon - gap }}
                />
              </Children>
            </Entity>
          </Children>
        </Entity>
        <OptionList
          id={`${id}/options`}
          options={options.filter((option) => filter(option, query))}
          selected={selected ? [selected.key] : []}
          onPick={pick}
          emptyLabel={emptyLabel}
          {...(loading === undefined ? {} : { loading })}
          {...(maxRows === undefined ? {} : { maxRows })}
        />
      </SelectList>
    </SelectTrigger>
  );
}
