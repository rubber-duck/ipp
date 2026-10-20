import {
  KitLayout,
  useGuiKitLayout,
  type GuiKitLayoutProps,
  LAYOUT_LEAF,
} from "../layout.js";

import { useEntityId } from "../../scope/identity.js";
/**
 * An autocomplete: a text field with the application's suggestions in an
 * option list below it. The typed text is the field's value; suggestions
 * never rewrite it until one is accepted.
 *
 * Typing reports the text through `onInputChange` and opens the list
 * whenever the application has suggestions for it, or `loading` shows the
 * spinner's row; the list closes while there are none. The arrows move the
 * active suggestion while focus and the caret stay in the field. Enter with
 * an active suggestion, or a press on one, accepts it: the field takes its
 * label, the list closes and `onSelect` reports its key. Enter without an
 * active suggestion commits the typed text through `onCommit` and closes the
 * list. Escape, Tab or a press outside closes the list and leaves the text;
 * typing opens it again.
 *
 * The application owns the suggestions, typically fetched or filtered from
 * `onInputChange`; the field owns its text, which starts as `defaultInputValue`.
 */
import { useLayoutEffect, useRef, type Ref } from "react";
import { PresentationGate, Children, Entity } from "../../components.js";
import { Skin } from "../../gui/theme.js";
import { TextInput } from "../../gui/controls.js";
import { Style, Behavior, Font } from "../../gui/components.js";
import type {
  GuiSubmitEvent,
  GuiTextCommitListener,
  GuiVisibleChangeListener,
} from "../../gui/callbacks.js";
import type { GuiControlHandle } from "../../gui/control-ref.js";
import { useGuiKit } from "../kit.js";

import { OptionList, type SelectOption } from "./option-list.js";
import { useOverlayOpen } from "../overlays/overlay.js";
import { useTextValue } from "../values/text-value.js";
import {
  SELECT_WIDTH,
  SelectList,
  useConnectedSelectSkin,
} from "./select-trigger.js";

export interface AutocompleteProps extends GuiKitLayoutProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  /** Symbolic id of the field; the list and its rows extend it. */
  readonly id?: string;
  /** The application's suggestions for the current text. */
  readonly suggestions: readonly SelectOption[];
  /** The field's text changed: typed, or an accepted suggestion's label. */
  readonly onInputChange?: (text: string) => void;
  /** A suggestion was accepted: its key. */
  readonly onSelect?: (key: string) => void;
  /** Enter without an active suggestion: the typed text. */
  readonly onCommit?: (text: string) => void;
  /** Application text to synchronize with the runtime; pair with onInputChange. */
  readonly inputValue?: string;
  /** Initial text while inputValue is omitted. */
  readonly defaultInputValue?: string;
  readonly placeholder?: string;
  /** The field's semantic name, such as Destination. */
  readonly label?: string;
  /** Show the loading row with this text, such as Loading…. */
  readonly loading?: string;
  readonly disabled?: boolean;
  /** Rows the list shows before it scrolls; six by default. */
  readonly maxRows?: number;
  /** The field's control handle. */
  readonly ref?: Ref<GuiControlHandle>;
}

export function Autocomplete(props: AutocompleteProps) {
  const {
    id: providedId,
    layer = 0,
    suggestions,
    onInputChange,
    onSelect,
    onCommit,
    inputValue,
    defaultInputValue = "",
    placeholder = "",
    label,
    loading,
    disabled = false,
    maxRows,
    ref,
    layout,
  } = useGuiKitLayout(props);
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  const t = kit.tokens;
  // Whether the user wants the list: opened by typing, closed by the
  // runtime, an acceptance or a commit.
  const overlay = useOverlayOpen();
  const shown =
    overlay.open && (suggestions.length > 0 || loading !== undefined);
  // The visibility last declared, so the report of the list hiding for
  // lack of suggestions is not taken for the runtime closing it.
  const declared = useRef(shown);
  useLayoutEffect(() => {
    declared.current = shown;
  }, [shown]);
  const follow: GuiVisibleChangeListener = (event) => {
    if (!event.value && !declared.current) return;
    overlay.onVisibleChange(event);
  };

  const text = useTextValue(inputValue, defaultInputValue, onInputChange, ref);
  const written = useRef<string | undefined>(undefined);

  const changed: GuiTextCommitListener = (event) => {
    const accepted = event.value === written.current;
    written.current = undefined;
    if (!text.changed(event)) return;
    if (!accepted) overlay.setOpen(true);
  };
  const accept = (key: string) => {
    if (!overlay.open) return;
    overlay.setOpen(false);
    const suggestion = suggestions.find((option) => option.key === key);
    if (suggestion) {
      written.current = suggestion.label;
      void text.handle.current
        ?.action({ kind: "text", value: suggestion.label })
        .then((outcome) => {
          if (!outcome.ok && written.current === suggestion.label)
            written.current = undefined;
        })
        .catch(() => {
          if (written.current === suggestion.label) written.current = undefined;
        });
    }
    onSelect?.(key);
  };
  const commit = (event: GuiSubmitEvent) => {
    overlay.setOpen(false);
    onCommit?.(event.value);
  };

  const connectedSkin = useConnectedSelectSkin(shown);
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style layer={layer} />
      <KitLayout
        kind={LAYOUT_LEAF}
        width={kit.unit(SELECT_WIDTH)}
        height={kit.unit(t.controlHeight)}
        align_y={0}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Behavior
        enabled={!disabled}
        {...(label === undefined ? {} : { semantic_label: label })}
      />
      {kit.controlTheme("textInput") && (
        <Skin theme={kit.controlTheme("textInput")} {...connectedSkin} />
      )}
      <TextInput
        placeholder={placeholder}
        text={text.initial}
        onTextCommit={changed}
        onSubmit={commit}
        ref={text.ref}
      />
      <Children>
        <SelectList id={`${id}/list`} open={shown} onVisibleChange={follow}>
          <OptionList
            id={`${id}/suggestions`}
            options={suggestions}
            onPick={accept}
            {...(loading === undefined ? {} : { loading })}
            {...(maxRows === undefined ? {} : { maxRows })}
          />
        </SelectList>
      </Children>
    </Entity>
  );
}
