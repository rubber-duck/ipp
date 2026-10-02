/**
 * A small observable record for application state that more than one React
 * root shows. The GUI page's sidebar renders through React DOM and its panel
 * through the panel World's own React root, so the page keeps its state here
 * rather than in either tree's component state. Each component selects the
 * values it shows with `useStoreValue` and re-renders only when they change:
 * moving GAIN re-renders the gain readouts, the waveform, the node rows and
 * the projector, and no other part of the panel.
 */
import { useSyncExternalStore } from "react";

export class Store<State extends object> {
  private state: State;
  private readonly listeners = new Set<() => void>();

  constructor(initial: State) {
    this.state = initial;
  }

  /** The current state, for event handlers and operations; components select
   * from it with `useStoreValue`. */
  get current(): State {
    return this.state;
  }

  /** Merge `change` into the state; a change that sets every field to its
   * current value notifies nobody. */
  update(change: Partial<State> | ((state: State) => Partial<State>)): void {
    const patch = typeof change === "function" ? change(this.state) : change;
    const changed = (Object.keys(patch) as (keyof State)[]).some(
      (key) => !Object.is(this.state[key], patch[key]),
    );
    if (!changed) return;
    this.state = { ...this.state, ...patch };
    for (const listener of this.listeners) listener();
  }

  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
}

/**
 * The value `select` reads from the store; the component re-renders only when
 * that value changes. `select` returns a stored field, a primitive computed
 * from fields or a constant, never a new object or array: an object built
 * from the state is built in the component from the values it selects.
 */
export function useStoreValue<State extends object, Value>(
  store: Store<State>,
  select: (state: State) => Value,
): Value {
  return useSyncExternalStore(store.subscribe, () => select(store.current));
}
