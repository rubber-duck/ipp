import type { GuiInputRoutingOutcome } from "@ipp/client";
import type { BrowserGuiInputCommand, GuiPointerButton } from "./input.js";

/** Admission gate for scene gestures sharing a canvas with runtime GUI input.
 *
 * Pointer presses and wheel samples resolve true only when the correlated
 * runtime reply says that the exact input missed every panel. Callers may
 * buffer DOM motion while admission is pending; aborting the signal fences a
 * cancelled gesture or an old canvas/session generation.
 */
export interface GuiUnhandledInputGate {
  pointerDown(
    pointer: number,
    button: GuiPointerButton,
    signal: AbortSignal,
  ): Promise<boolean>;
  scroll(signal: AbortSignal): Promise<boolean>;
}

interface GateWaiter {
  readonly kind: "pointerDown" | "scroll";
  readonly pointer?: number;
  readonly button?: GuiPointerButton;
  readonly signal: AbortSignal;
  readonly resolve: (admitted: boolean) => void;
  entry: GateEntry | undefined;
  abort: (() => void) | undefined;
  done: boolean;
}

interface GateEntry {
  readonly kind: "pointerDown" | "scroll";
  readonly pointer?: number;
  readonly button?: GuiPointerButton;
  waiter: GateWaiter | undefined;
  done: boolean;
}

interface GateState {
  readonly entries: GateEntry[];
  readonly waiters: GateWaiter[];
  closed: boolean;
  generation: number;
}

export interface GuiUnhandledInputSubmission {
  readonly generation: number;
  readonly entry: GateEntry;
}

const states = new WeakMap<GuiUnhandledInputGate, GateState>();

function removeEntry(state: GateState, entry: GateEntry): void {
  const index = state.entries.indexOf(entry);
  if (index >= 0) state.entries.splice(index, 1);
}

function settleWaiter(
  state: GateState,
  waiter: GateWaiter,
  admitted: boolean,
): void {
  if (waiter.done) return;
  waiter.done = true;
  if (waiter.abort) waiter.signal.removeEventListener("abort", waiter.abort);
  const waiterIndex = state.waiters.indexOf(waiter);
  if (waiterIndex >= 0) state.waiters.splice(waiterIndex, 1);
  if (waiter.entry) {
    removeEntry(state, waiter.entry);
    waiter.entry.waiter = undefined;
    waiter.entry = undefined;
  }
  waiter.resolve(admitted);
}

function matches(waiter: GateWaiter, entry: GateEntry): boolean {
  return (
    waiter.kind === entry.kind &&
    (waiter.kind === "scroll" ||
      (waiter.pointer === entry.pointer && waiter.button === entry.button))
  );
}

function match(state: GateState): void {
  for (const waiter of [...state.waiters]) {
    if (waiter.signal.aborted || waiter.entry) continue;
    const entry = state.entries.find(
      (candidate) =>
        !candidate.waiter && !candidate.done && matches(waiter, candidate),
    );
    if (!entry) continue;
    waiter.entry = entry;
    entry.waiter = waiter;
  }
}

/** Create a reusable runtime-authoritative GUI-to-scene input gate. */
export function createGuiUnhandledInputGate(): GuiUnhandledInputGate {
  const state: GateState = {
    entries: [],
    waiters: [],
    closed: false,
    generation: 0,
  };
  const wait = (
    description: Omit<GateWaiter, "resolve" | "done" | "entry" | "abort">,
  ): Promise<boolean> =>
    new Promise((resolve) => {
      if (state.closed || description.signal.aborted) {
        resolve(false);
        return;
      }
      const waiter: GateWaiter = {
        ...description,
        resolve,
        entry: undefined,
        abort: undefined,
        done: false,
      };
      waiter.abort = () => settleWaiter(state, waiter, false);
      description.signal.addEventListener("abort", waiter.abort, {
        once: true,
      });
      state.waiters.push(waiter);
      match(state);
    });
  const gate: GuiUnhandledInputGate = {
    pointerDown(pointer, button, signal) {
      return wait({ kind: "pointerDown", pointer, button, signal });
    },
    scroll(signal) {
      return wait({ kind: "scroll", signal });
    },
  };
  states.set(gate, state);
  return gate;
}

/** @internal Record one exact browser event before its immediate submission. */
export function trackUnhandledInputGate(
  gate: GuiUnhandledInputGate | undefined,
  generation: number,
  command: BrowserGuiInputCommand,
): GuiUnhandledInputSubmission | undefined {
  if (
    gate === undefined ||
    (command.kind !== "pointerDown" && command.kind !== "scroll")
  )
    return undefined;
  const state = states.get(gate);
  if (state === undefined || state.closed || state.generation !== generation)
    return undefined;
  const entry: GateEntry =
    command.kind === "pointerDown"
      ? {
          kind: command.kind,
          pointer: command.pointer,
          button: command.button,
          waiter: undefined,
          done: false,
        }
      : { kind: command.kind, waiter: undefined, done: false };
  state.entries.push(entry);
  match(state);
  return { generation, entry };
}

/** @internal Settle the exact entry from its correlated routing reply. */
export function settleUnhandledInputGateSubmission(
  gate: GuiUnhandledInputGate | undefined,
  submission: GuiUnhandledInputSubmission | undefined,
  outcome?: GuiInputRoutingOutcome,
): void {
  if (gate === undefined || submission === undefined) return;
  const state = states.get(gate);
  const { entry, generation } = submission;
  if (
    state === undefined ||
    state.closed ||
    state.generation !== generation ||
    entry.done
  )
    return;
  entry.done = true;
  const admitted = outcome?.unhandled?.kind === "noPanelHit";
  if (entry.waiter) settleWaiter(state, entry.waiter, admitted);
  else removeEntry(state, entry);
}

/** @internal Attach a gate to one sink/client generation. */
export function openUnhandledInputGate(
  gate: GuiUnhandledInputGate | undefined,
): number {
  const state = gate === undefined ? undefined : states.get(gate);
  if (!state) return 0;
  if (!state.closed) {
    for (const waiter of [...state.waiters]) settleWaiter(state, waiter, false);
    state.entries.length = 0;
  }
  state.generation += 1;
  state.closed = false;
  return state.generation;
}

/** @internal Fence a detached sink/client generation and all pending work. */
export function closeUnhandledInputGate(
  gate: GuiUnhandledInputGate | undefined,
  generation: number,
): void {
  const state = gate === undefined ? undefined : states.get(gate);
  if (state === undefined || state.closed || state.generation !== generation)
    return;
  state.closed = true;
  for (const waiter of [...state.waiters]) settleWaiter(state, waiter, false);
  state.entries.length = 0;
}
