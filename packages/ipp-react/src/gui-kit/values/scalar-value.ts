/**
 * The values of the kit's scalar composites, the sliders, knob and numeric
 * stepper, and their text. The runtime owns a control's values: it writes
 * them as drags, keys, the wheel, step parts and committed entries change
 * them, and the composite reports each committed change through `onChange`,
 * from the control's `onScalarCommit`, so its readouts follow the committed
 * values without a tween. A range's two values arrive in one event and are
 * reported once.
 *
 * The control declares its values once, when it mounts, from the values the
 * composite shows then, and never again: a declaration that followed the
 * reported values would write each report back a client round trip later,
 * by which time a drag may have moved on, and the stale write would pull the
 * thumb back. A new `value` the runtime did not report is the application's
 * and is written through the control handle by compare-and-set from the
 * values the runtime last reported, so a drag that moved on in the meantime
 * wins. A range's two fields are written one at a time, the one moving away
 * from the other first, so neither write inverts the range, and the state
 * between the two writes is not reported.
 *
 * Values compare as the runtime stores them, in single precision.
 */
import { useEffect, useRef, useState, type RefObject } from "react";
import type { GuiScalarEvent } from "../../gui/callbacks.js";
import type { GuiControlHandle, GuiControlRef } from "../../gui/control-ref.js";

/** A control's values: its value, or a range's lower and upper values. */
export type ScalarValues = readonly number[];

export interface ScalarValue {
  /** The values the composite shows: the application's, or its own. */
  readonly current: ScalarValues;
  /** Props of the composite's Slider or numeric TextInput. */
  readonly control: {
    /** The value, and a range's upper value, as they were at mount. */
    readonly value: number;
    readonly upper?: number;
    readonly onScalarCommit: (event: GuiScalarEvent) => void;
    readonly ref: (handle: GuiControlHandle | null) => void;
  };
}

/** Whether two value lists hold the same single-precision values. */
function same(left: ScalarValues, right: ScalarValues): boolean {
  return (
    left.length === right.length &&
    left.every(
      (value, index) => Math.fround(value) === Math.fround(right[index]!),
    )
  );
}

/**
 * The values of a scalar composite: the application's `value`, or its own
 * from `defaultValue`, reported to `onChange` as the runtime commits them.
 * `ref` also receives the control's handle.
 */
export function useScalarValue(
  value: ScalarValues | undefined,
  defaultValue: ScalarValues,
  onChange: ((values: ScalarValues) => void) | undefined,
  ref: GuiControlRef | undefined,
): ScalarValue {
  const [own, setOwn] = useState(defaultValue);
  const current = value ?? own;
  const [mounted] = useState(current);
  // The values the runtime holds as far as the composite knows: the last
  // ones reported or written.
  const held = useRef(current);
  // The state between the two writes of a range, which is not reported.
  const between = useRef<ScalarValues | undefined>(undefined);
  const handle = useRef<GuiControlHandle | null>(null);
  // A write waiting for the handle, which publishes after the declaration.
  const waiting = useRef<{ from: ScalarValues; to: ScalarValues } | undefined>(
    undefined,
  );
  const outer = useRef(ref);
  outer.current = ref;

  const write = (from: ScalarValues, to: ScalarValues) => {
    const target = handle.current;
    if (!target) {
      waiting.current = { from, to };
      return;
    }
    waiting.current = undefined;
    const set = (field: "value" | "upper", index: number) => {
      if (Math.fround(from[index]!) === Math.fround(to[index]!)) return false;
      void target
        .compareAndSet(field, from[index]!, to[index]!)
        .catch(() => false);
      return true;
    };
    if (to.length === 1) {
      set("value", 0);
      return;
    }
    const upperFirst = to[1]! >= from[1]!;
    const first = upperFirst ? set("upper", 1) : set("value", 0);
    const second = upperFirst ? set("value", 0) : set("upper", 1);
    between.current =
      first && second
        ? upperFirst
          ? [from[0]!, to[1]!]
          : [to[0]!, from[1]!]
        : undefined;
  };

  // A value the runtime did not report is the application's: write it.
  const key = value?.join(" ");
  useEffect(() => {
    if (value === undefined || same(value, held.current)) return;
    const from = held.current;
    held.current = value;
    write(from, value);
    // The key carries the values; a new array of the same values is no change.
  }, [key]);

  const [attach] = useState(() => (next: GuiControlHandle | null) => {
    handle.current = next;
    const user = outer.current;
    if (typeof user === "function") user(next);
    else if (user) (user as RefObject<GuiControlHandle | null>).current = next;
    if (next && waiting.current)
      write(waiting.current.from, waiting.current.to);
  });

  // Retained declarations call the latest render's callbacks, so this one
  // sees the current value and listener.
  const onScalarCommit = (event: GuiScalarEvent) => {
    const values =
      event.upper === undefined ? [event.value] : [event.value, event.upper];
    const step = between.current;
    between.current = undefined;
    if (same(values, held.current) || (step && same(values, step))) return;
    held.current = values;
    if (value === undefined) setOwn(values);
    onChange?.(values);
  };

  return {
    current,
    control: {
      value: mounted[0]!,
      ...(mounted.length > 1 ? { upper: mounted[1]! } : {}),
      onScalarCommit,
      ref: attach,
    },
  };
}

/**
 * The places after the decimal point that show multiples of `step` exactly,
 * up to six.
 */
export function decimalsOf(step: number): number {
  for (let places = 0; places < 6; places++) {
    const scaled = Math.abs(step) * 10 ** places;
    if (Math.abs(scaled - Math.round(scaled)) < 1e-6) return places;
  }
  return 6;
}

/**
 * The kit's default text of a value: fixed to `decimals` places, without a
 * negative zero, and with a plus sign on positive values when `signed`, as on
 * a range that runs below zero, whose positive side reads as a direction.
 */
export function formatValue(
  value: number,
  decimals: number,
  signed: boolean,
): string {
  const fixed = value.toFixed(decimals);
  if (/^-0(\.0*)?$/.test(fixed)) return fixed.slice(1);
  return signed && value > 0 && !/^0(\.0*)?$/.test(fixed) ? `+${fixed}` : fixed;
}

/**
 * The slider composites' default format: the decimals of `step`, or of a
 * hundredth of the range, the runtime's stride, while the step is zero; a
 * range that runs below zero signs its positive values.
 */
export function defaultFormat(
  min: number,
  max: number,
  step: number,
): (value: number) => string {
  const decimals = decimalsOf(step > 0 ? step : (max - min) / 100);
  return (value) => formatValue(value, decimals, min < 0);
}

/**
 * Formatted `text` with its units: a unit that begins with a letter follows
 * a space, as in `20 m`, and a symbol such as `%` or `°` follows directly.
 */
export function withUnits(text: string, units: string | undefined): string {
  if (!units) return text;
  return /^\p{L}/u.test(units) ? `${text} ${units}` : `${text}${units}`;
}
