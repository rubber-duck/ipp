/**
 * The colour picker's one colour and the presentations derived from it.
 *
 * The canonical colour is the colour control's HSVA (`GuiHsva`): hue in turns
 * from red, saturation and value of the HSV model on sRGB-encoded values, and
 * alpha, the runtime's linear coverage, which composites the colour over what
 * lies beneath in linear light rather than blending sRGB-encoded values as
 * CSS does. Red, green and blue are its sRGB bytes, 0 to 255; hex is
 * `#RRGGBB` in that order, with `AA`, the coverage as a byte, appended as
 * `#RRGGBBAA`. Every presentation converts from and back to the HSVA; none is
 * kept as a second authority.
 *
 * Converting HSVA to bytes and back is stable: the HSV of a byte colour,
 * stored in the control's single precision, rounds back to the same bytes, so
 * a typed hex reads back as itself. A grey, white or black byte colour has no
 * hue, and black no saturation either; converting one keeps the previous
 * colour's hue, and for black its saturation too, so the hue rail and the
 * field's marker do not jump when a colour passes through them.
 */
import { useEffect, useRef, useState, type RefObject } from "react";
import type { GuiControlEvent, GuiHsva } from "../../gui/callbacks.js";
import type { GuiControlHandle, GuiControlRef } from "../../gui/control-ref.js";
import type { GuiKitColor } from "../kit.js";

/** A colour's sRGB bytes, 0 to 255, and its coverage alpha, 0 to 1. */
export interface ColorRgba {
  readonly red: number;
  readonly green: number;
  readonly blue: number;
  readonly alpha: number;
}

/** The channels as the control stores them, in single precision. */
function stored(color: GuiHsva): GuiHsva {
  return {
    hue: Math.fround(color.hue),
    saturation: Math.fround(color.saturation),
    value: Math.fround(color.value),
    alpha: Math.fround(color.alpha),
  };
}

/** Whether two colours hold the same single-precision channels. */
export function sameColor(left: GuiHsva, right: GuiHsva): boolean {
  const [a, b] = [stored(left), stored(right)];
  return (
    a.hue === b.hue &&
    a.saturation === b.saturation &&
    a.value === b.value &&
    a.alpha === b.alpha
  );
}

/**
 * The sRGB bytes of `color`, unrounded, as the control stores it: each
 * channel the value less the saturation's share of what the hue leaves out.
 */
function channels(color: GuiHsva): readonly [number, number, number] {
  const { hue, saturation, value } = stored(color);
  const level = (offset: number) => {
    const turn = (((hue + offset) % 1) + 1) % 1;
    const pure = Math.min(Math.max(Math.abs(turn * 6 - 3) - 1, 0), 1);
    return (1 + (pure - 1) * saturation) * value * 255;
  };
  return [level(0), level(2 / 3), level(1 / 3)];
}

/** `color` as rounded sRGB bytes and its coverage. */
export function hsvaToRgba(color: GuiHsva): ColorRgba {
  const [red, green, blue] = channels(color).map(Math.round) as [
    number,
    number,
    number,
  ];
  return { red, green, blue, alpha: stored(color).alpha };
}

/**
 * The HSVA of sRGB bytes and coverage, each clamped to its range. A colour
 * without hue (grey, white or black) keeps `previous`'s hue, and black keeps
 * its saturation too.
 */
export function rgbaToHsva(
  { red, green, blue, alpha }: ColorRgba,
  previous?: GuiHsva,
): GuiHsva {
  const [r, g, b] = [red, green, blue].map(
    (byte) => Math.min(Math.max(byte, 0), 255) / 255,
  ) as [number, number, number];
  const high = Math.max(r, g, b);
  const low = Math.min(r, g, b);
  const span = high - low;
  let hue = previous?.hue ?? 0;
  if (span > 0) {
    const sixth =
      high === r
        ? (g - b) / span
        : high === g
          ? 2 + (b - r) / span
          : 4 + (r - g) / span;
    hue = (((sixth / 6) % 1) + 1) % 1;
  }
  const saturation = high > 0 ? span / high : (previous?.saturation ?? 0);
  return {
    hue,
    saturation,
    value: high,
    alpha: Math.min(Math.max(alpha, 0), 1),
  };
}

const byte = (value: number) =>
  Math.round(value).toString(16).padStart(2, "0").toUpperCase();

/**
 * `#RRGGBB` of `color`'s sRGB bytes, and `AA` after them when `alpha` and the
 * colour is not opaque.
 */
export function formatHex(color: GuiHsva, alpha = true): string {
  const { red, green, blue, alpha: coverage } = hsvaToRgba(color);
  const rgb = `#${byte(red)}${byte(green)}${byte(blue)}`;
  const aa = Math.round(coverage * 255);
  return alpha && aa < 255 ? `${rgb}${byte(aa)}` : rgb;
}

/**
 * The colour of `#RRGGBB` or `#RRGGBBAA` (case-insensitive, the `#`
 * optional, surrounding space ignored), keeping `previous`'s hue for a
 * colour without one and its alpha when `AA` is absent; nothing for any
 * other text.
 */
export function parseHex(
  text: string,
  previous?: GuiHsva,
): GuiHsva | undefined {
  const match = /^#?([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(text.trim());
  if (!match) return undefined;
  const digits = match[1]!;
  const at = (index: number) =>
    Number.parseInt(digits.slice(index, index + 2), 16);
  return rgbaToHsva(
    {
      red: at(0),
      green: at(2),
      blue: at(4),
      alpha:
        match[2] === undefined
          ? (previous?.alpha ?? 1)
          : Number.parseInt(match[2], 16) / 255,
    },
    previous,
  );
}

/** Linear RGBA of `color`, as theme rows author colours. */
export function linearColor(color: GuiHsva): GuiKitColor {
  const linear = (level: number) => {
    const encoded = level / 255;
    return encoded <= 0.04045
      ? encoded / 12.92
      : ((encoded + 0.055) / 1.055) ** 2.4;
  };
  const [red, green, blue] = channels(color).map(linear) as [
    number,
    number,
    number,
  ];
  return [red, green, blue, stored(color).alpha];
}

export interface ColorValue {
  /** The colour the picker shows: the application's, or its own. */
  readonly current: GuiHsva;
  /** Props of the picker's Color control. */
  readonly control: {
    /** The channels as they were at mount. */
    readonly hue: number;
    readonly saturation: number;
    readonly value: number;
    readonly alpha: number;
    readonly onColorCommit: (event: GuiControlEvent<GuiHsva>) => void;
    readonly ref: (handle: GuiControlHandle | null) => void;
  };
  /**
   * Set a colour entered in the picker, such as a typed hex or a preset: the
   * control's value action, whose commit is then reported as the runtime's.
   */
  enter(color: GuiHsva): void;
}

/**
 * The picker's colour: the application's `value`, or its own from
 * `defaultValue`, reported to `onChange` as the runtime commits it, from the
 * control's drags and keys and from the picker's own entries. The control
 * declares its channels once, when it mounts; a later colour the runtime did
 * not report is the application's and is set with the control's value
 * action, one write of all four channels, and not reported back. `ref` also
 * receives the control's handle.
 */
export function useColorValue(
  value: GuiHsva | undefined,
  defaultValue: GuiHsva,
  onChange: ((value: GuiHsva) => void) | undefined,
  ref: GuiControlRef | undefined,
): ColorValue {
  const [own, setOwn] = useState(defaultValue);
  const current = value ?? own;
  const [mounted] = useState(current);
  // The colour the runtime holds as far as the picker knows: the last one
  // reported or the application's last write.
  const held = useRef(current);
  const handle = useRef<GuiControlHandle | null>(null);
  // A write waiting for the handle, which publishes after the declaration.
  const waiting = useRef<GuiHsva | undefined>(undefined);
  const outer = useRef(ref);
  outer.current = ref;

  const set = (color: GuiHsva) => {
    const target = handle.current;
    if (!target) {
      waiting.current = color;
      return;
    }
    waiting.current = undefined;
    const { hue, saturation, value: level, alpha } = color;
    void target
      .action({ kind: "color", value: [hue, saturation, level, alpha] })
      .catch(() => {});
  };

  // A colour the runtime did not report is the application's: set it.
  const key = value && JSON.stringify(stored(value));
  useEffect(() => {
    if (value === undefined || sameColor(value, held.current)) return;
    held.current = value;
    set(value);
    // The key carries the channels; a new object of the same colour is no change.
  }, [key]);

  const [attach] = useState(() => (next: GuiControlHandle | null) => {
    handle.current = next;
    const user = outer.current;
    if (typeof user === "function") user(next);
    else if (user) (user as RefObject<GuiControlHandle | null>).current = next;
    if (next && waiting.current) set(waiting.current);
  });

  return {
    current,
    control: {
      ...mounted,
      // Retained declarations call the latest render's callbacks.
      onColorCommit: (event) => {
        if (sameColor(event.value, held.current)) return;
        held.current = event.value;
        if (value === undefined) setOwn(event.value);
        onChange?.(event.value);
      },
      ref: attach,
    },
    enter(color) {
      if (!sameColor(color, held.current)) set(color);
    },
  };
}
