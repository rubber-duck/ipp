/**
 * The selection of a choice composite: radio group, segmented control, tabs
 * and tree view. Its items are Buttons below a `GuiGroup` that selects, so
 * the selection lives in the items' `selected` fields and the runtime writes
 * it as soon as an item is activated, or, in a group whose selection
 * follows, as soon as arrows move to one. The composite reports each item the
 * runtime selects through `onChange`, from the item's `onSelectedChange`.
 *
 * An item declares its `selected` field once, when it mounts, from the
 * selection the composite shows then, and never again: a declaration that
 * followed the reported selection would write each report back after a
 * client round trip, by which time arrows may have moved the selection on,
 * and the stale write would select the earlier item again. A new `value` the
 * runtime did not report, and a part of the composite that selects without
 * being the item, such as a radio option's label, write the item's field
 * through its control handle instead.
 */
import { useEffect, useRef, useState } from "react";
import type { GuiAction } from "@ipp/client";
import type { GuiControlEvent } from "../gui/callbacks.js";
import type { GuiControlHandle } from "../gui/control-ref.js";

/** `GuiGroup.axis` values. */
export const GROUP_HORIZONTAL = 0;
export const GROUP_VERTICAL = 1;

/** `GuiGroup.selection` values: activation selects; arrows select too. */
export const SELECT_SINGLE = 1;
export const SELECT_FOLLOW = 2;

/** What the composite's application gives for its selection. */
export interface ChoiceProps<Key extends string = string> {
  /** The selected item's key, controlled; pair it with `onChange`. */
  readonly value?: Key;
  /** The initially selected item while `value` is omitted. */
  readonly defaultValue?: Key;
  /** The runtime selected an item: activation, or arrows where they select. */
  readonly onChange?: (value: Key) => void;
}

/** The Button props through which an item carries and reports its selection. */
export interface ChoiceItem {
  readonly selected: boolean;
  readonly onSelectedChange: (event: GuiControlEvent<boolean>) => void;
  readonly ref: (handle: GuiControlHandle | null) => void;
}

export interface Choice<Key extends string = string> {
  /** The selection the composite shows: the application's value, or its own. */
  readonly current: Key | undefined;
  /** The props of item `key`'s Button. */
  item(key: Key): ChoiceItem;
  /** Select item `key` for a part of the composite that is not the item. */
  select(key: Key): void;
  /**
   * Apply `action` to item `key`, as soon as its handle is acknowledged when
   * the item has only just been declared; a later call replaces a waiting one.
   */
  act(key: Key, action: GuiAction): void;
}

/** The selection of a choice composite whose items have keys of type `Key`. */
export function useChoice<Key extends string>({
  value,
  defaultValue,
  onChange,
}: ChoiceProps<Key>): Choice<Key> {
  const [own, setOwn] = useState(defaultValue);
  const current = value ?? own;
  // The selection the runtime holds as far as the composite knows: the last
  // one reported or written.
  const held = useRef(current);
  const handles = useRef(new Map<Key, GuiControlHandle>());
  const refs = useRef(new Map<Key, ChoiceItem["ref"]>());
  const waiting = useRef<{ key: Key; action: GuiAction } | undefined>(
    undefined,
  );

  // A value the runtime did not report is the application's: write it.
  useEffect(() => {
    if (value === undefined || value === held.current) return;
    held.current = value;
    write(handles.current.get(value));
  }, [value]);

  // Retained declarations call the latest render's callbacks, so this one
  // sees the current value and listener.
  const report = (key: Key, event: GuiControlEvent<boolean>) => {
    if (!event.value || key === held.current) return;
    held.current = key;
    if (value === undefined) setOwn(key);
    onChange?.(key);
  };

  return {
    current,
    item(key) {
      let ref = refs.current.get(key);
      if (!ref) {
        ref = (handle) => {
          if (!handle) {
            handles.current.delete(key);
            refs.current.delete(key);
            return;
          }
          handles.current.set(key, handle);
          if (waiting.current?.key !== key) return;
          apply(handle, waiting.current.action);
          waiting.current = undefined;
        };
        refs.current.set(key, ref);
      }
      return {
        selected: key === current,
        onSelectedChange: (event) => report(key, event),
        ref,
      };
    },
    select(key) {
      write(handles.current.get(key));
    },
    act(key, action) {
      const handle = handles.current.get(key);
      waiting.current = handle ? undefined : { key, action };
      if (handle) apply(handle, action);
    },
  };
}

/**
 * Select the item whose handle is `handle`; the group clears the others. A
 * handle that is gone with its item has nothing left to select or act on.
 */
function write(handle: GuiControlHandle | undefined): void {
  void handle?.compareAndSet("selected", false, true).catch(() => {});
}

function apply(handle: GuiControlHandle, action: GuiAction): void {
  void handle.action(action).catch(() => {});
}

/** `selected` as it was when the calling item mounted. */
export function useMountSelection(selected: boolean): boolean {
  return useState(selected)[0];
}
