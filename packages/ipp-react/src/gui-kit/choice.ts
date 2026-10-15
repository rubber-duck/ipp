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
 * runtime did not report is application intent. Rejected edits converge after
 * the callback's React commit through conditional writes. Pending writes are
 * separate from aggregate observations, and item handles fence every write.
 * A radio label's explicit activation still selects its native item.
 */
import { useRef, useState } from "react";
import {
  useValueSynchronization,
  type ValueChangeHandler,
} from "./values/value-synchronization.js";
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
  /** The runtime selected an item. Return asynchronous acceptance so pending edits are retained. */
  readonly onChange?: ValueChangeHandler<Key>;
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
  const handles = useRef(new Map<Key, GuiControlHandle>());
  const refs = useRef(new Map<Key, ChoiceItem["ref"]>());
  const observed = useRef<Key | undefined>(undefined);
  const observedTick = useRef(-1n);
  const admitted = useRef(false);
  const waiting = useRef<{ key: Key; action: GuiAction } | undefined>(
    undefined,
  );
  // The group owns one aggregate observation. Individual handle churn does
  // not invalidate reports from its other still-live items.
  const synchronization = useValueSynchronization<Key | undefined>({
    value,
    defaultValue,
    onChange: (key) => {
      if (key !== undefined) return onChange?.(key);
    },
    same: (left, right) => left === right,
    identity: () => handles.current,
    write: async (_from, to, context) => {
      if (to === undefined) return;
      const target = handles.current.get(to);
      if (!target || !context.isCurrent()) return;
      await context.trackEcho(to, async () => {
        if (!context.isCurrent() || handles.current.get(to) !== target)
          return false;
        return target.compareAndSet("selected", false, true);
      });
    },
  });
  const current =
    value !== undefined
      ? value
      : admitted.current &&
          synchronization.current !== undefined &&
          !handles.current.has(synchronization.current)
        ? undefined
        : synchronization.current;
  const report = (key: Key, event: GuiControlEvent<boolean>) => {
    if (event.tick < observedTick.current) return;
    observedTick.current = event.tick;
    if (event.value) observed.current = key;
    else if (observed.current === key) observed.current = undefined;
    else if (!event.initial) return;
    synchronization.observe(observed.current, event);
  };

  return {
    current,
    item(key) {
      let ref = refs.current.get(key);
      if (!ref) {
        ref = (handle) => {
          if (!handle) {
            const previous = handles.current.get(key);
            handles.current.delete(key);
            if (observed.current === key) observed.current = undefined;
            if (previous) synchronization.retry();
            return;
          }
          const previous = handles.current.get(key);
          admitted.current = true;
          handles.current.set(key, handle);
          if (previous !== handle) synchronization.retry();
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
