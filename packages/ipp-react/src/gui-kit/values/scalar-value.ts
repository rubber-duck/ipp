/** Native scalar observations drive the view; controlled edits use acknowledged conditional writes. */
import { useRef } from "react";
import {
  useValueSynchronization,
  type ValueChangeHandler,
} from "./value-synchronization.js";
import { useComposedControlRef } from "../../gui/control-refs.js";
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
    readonly ref: GuiControlRef;
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
  onChange: ValueChangeHandler<ScalarValues> | undefined,
  ref: GuiControlRef | undefined,
): ScalarValue {
  const handle = useRef<GuiControlHandle | null>(null);
  const state = useValueSynchronization({
    value,
    defaultValue,
    onChange,
    same,
    identity: () => handle.current,
    write: async (from, to, context) => {
      const target = handle.current;
      if (!target || from.length !== to.length) return;
      let intermediate = from;
      const fields: readonly (readonly ["value" | "upper", number])[] =
        to.length === 1
          ? [["value", 0]]
          : to[1]! >= from[1]!
            ? [
                ["upper", 1],
                ["value", 0],
              ]
            : [
                ["value", 0],
                ["upper", 1],
              ];
      for (const [field, index] of fields) {
        if (!context.isCurrent() || handle.current !== target) return;
        if (Math.fround(from[index]!) === Math.fround(to[index]!)) continue;
        const candidate = intermediate.map((value, at) =>
          at === index ? to[index]! : value,
        );
        const accepted = await context.trackEcho(candidate, () =>
          target.compareAndSet(field, from[index]!, to[index]!),
        );
        if (!accepted) return;
        intermediate = candidate;
      }
    },
  });
  const attach = useComposedControlRef(handle, ref, state.notifyReady);
  return {
    current: state.current,
    control: {
      value: state.initial[0]!,
      ...(state.initial.length > 1 ? { upper: state.initial[1]! } : {}),
      onScalarCommit: (event: GuiScalarEvent) => {
        state.observe(
          event.upper === undefined
            ? [event.value]
            : [event.value, event.upper],
          event,
        );
      },
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
