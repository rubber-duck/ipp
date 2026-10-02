/**
 * The projection's tuning: what the CONTROLS and COLOUR tabs, the scope's
 * popover and the scene tree set, held in the page's state beside the
 * station. Each setting reaches the scene, the scope or the readouts:
 *
 * - BEAM scales the beam's energy and LIGHT the studio key and fill lights.
 * - OFFSET moves both traces up or down the scope, and SWEEP bounds the band
 *   that crosses the scope while SCAN runs; RATE sets the scan's speed.
 * - CHANNELS switch the beam, the dust and the two studio lights on and off.
 * - PRESET sets all of those at once.
 * - The COLOUR tab's colour is the projection's: the projector's light, lens,
 *   beam, dust and metal accent. Choosing an ACCENT sets it to the accent's
 *   colour.
 * - The scope popover's GRID and SWEEP choose the scope paint's pattern and
 *   whether the band runs.
 * - The scene tree's selection names a scene node in the readouts and
 *   brightens it.
 */
import { useMemo } from "react";
import type { Store } from "./store.js";

export type ScanRate = "slow" | "normal" | "fast";

/** Scan speeds: the scan and sweep clips' playback speed. */
export const SCAN_RATES: readonly {
  readonly key: ScanRate;
  readonly label: string;
  readonly speed: number;
}[] = [
  { key: "slow", label: "SLOW", speed: 0.5 },
  { key: "normal", label: "NORMAL", speed: 1 },
  { key: "fast", label: "FAST", speed: 2 },
];

export type Channel = "beam" | "dust" | "key" | "fill";

export const CHANNELS: readonly {
  readonly key: Channel;
  readonly label: string;
}[] = [
  { key: "beam", label: "BEAM" },
  { key: "dust", label: "DUST" },
  { key: "key", label: "KEY" },
  { key: "fill", label: "FILL" },
];

export type ScopeGrid = "lines" | "scanlines" | "clean";

export const SCOPE_GRIDS: readonly {
  readonly key: ScopeGrid;
  readonly label: string;
}[] = [
  { key: "lines", label: "LINES" },
  { key: "scanlines", label: "SCANLINES" },
  { key: "clean", label: "CLEAN" },
];

/** A colour as the colour control holds it: HSV on sRGB-encoded values, hue in turns. */
export interface ProjectionColor {
  readonly hue: number;
  readonly saturation: number;
  readonly value: number;
}

export interface Tuning {
  /** Beam energy in percent of the authored beam. */
  readonly beam: number;
  /** Studio light level in percent; 50 is the authored level. */
  readonly light: number;
  /** Trace offset in percent of the scope's half height, up positive. */
  readonly offset: number;
  /** The sweep band's travel in percent of the scope's width. */
  readonly sweep: readonly [number, number];
  readonly rate: ScanRate;
  /** The last preset picked, until a setting it fixed changes. */
  readonly preset: string | undefined;
  readonly channels: readonly Channel[];
  readonly grid: ScopeGrid;
  /** Whether the sweep band runs while SCAN does. */
  readonly sweepShown: boolean;
  /** The scene tree's selected node. */
  readonly focus: string | undefined;
  readonly color: ProjectionColor;
}

/** The settings a preset fixes. */
type PresetValues = Pick<
  Tuning,
  "beam" | "light" | "offset" | "sweep" | "rate" | "channels"
>;

export const PRESETS: readonly {
  readonly key: string;
  readonly label: string;
  readonly values: PresetValues;
}[] = [
  {
    key: "survey",
    label: "SURVEY",
    values: {
      beam: 100,
      light: 50,
      offset: 0,
      // At first show the band crosses the whole scope, edge to edge.
      sweep: [0, 100],
      rate: "normal",
      channels: ["beam", "dust", "key", "fill"],
    },
  },
  {
    key: "relay",
    label: "RELAY",
    values: {
      beam: 160,
      light: 35,
      offset: 10,
      sweep: [40, 95],
      rate: "fast",
      channels: ["beam", "dust", "key"],
    },
  },
  {
    key: "quiet",
    label: "QUIET",
    values: {
      beam: 40,
      light: 70,
      offset: -10,
      sweep: [10, 40],
      rate: "slow",
      channels: ["key", "fill"],
    },
  },
  {
    key: "storm",
    label: "STORM",
    values: {
      beam: 190,
      light: 20,
      offset: 25,
      sweep: [0, 100],
      rate: "fast",
      channels: ["beam", "dust"],
    },
  },
  {
    key: "night",
    label: "NIGHT",
    values: {
      beam: 70,
      light: 10,
      offset: 0,
      sweep: [30, 70],
      rate: "slow",
      channels: ["beam", "dust"],
    },
  },
  {
    key: "beacon",
    label: "BEACON",
    values: {
      beam: 130,
      light: 60,
      offset: -25,
      sweep: [45, 55],
      rate: "normal",
      channels: ["beam", "key", "fill"],
    },
  },
];

/** The settings at first show: the SURVEY preset's, without naming it. */
export function initialTuning(color: ProjectionColor): Tuning {
  return {
    ...PRESETS[0]!.values,
    preset: undefined,
    grid: "lines",
    sweepShown: true,
    focus: undefined,
    color,
  };
}

/** sRGB encoding of a linear channel, and back. */
function encode(linear: number): number {
  const clamped = Math.min(Math.max(linear, 0), 1);
  return clamped <= 0.0031308
    ? clamped * 12.92
    : 1.055 * clamped ** (1 / 2.4) - 0.055;
}

function decode(encoded: number): number {
  const clamped = Math.min(Math.max(encoded, 0), 1);
  return clamped <= 0.04045
    ? clamped / 12.92
    : ((clamped + 0.055) / 1.055) ** 2.4;
}

/** Linear RGB of a projection colour. */
export function linearColor({
  hue,
  saturation,
  value,
}: ProjectionColor): readonly [number, number, number] {
  const channel = (offset: number) => {
    const turn = (((hue + offset) % 1) + 1) % 1;
    const pure = Math.min(Math.max(Math.abs(turn * 6 - 3) - 1, 0), 1);
    return decode((1 + (pure - 1) * saturation) * value);
  };
  return [channel(0), channel(2 / 3), channel(1 / 3)];
}

/** The projection colour of a linear RGB colour. */
export function projectionColor(
  linear: readonly [number, number, number],
): ProjectionColor {
  const [r, g, b] = linear.map(encode) as [number, number, number];
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const chroma = max - min;
  let hue = 0;
  if (chroma > 0) {
    if (max === r) hue = ((g - b) / chroma + 6) % 6;
    else if (max === g) hue = (b - r) / chroma + 2;
    else hue = (r - g) / chroma + 4;
  }
  return {
    hue: hue / 6,
    saturation: max > 0 ? chroma / max : 0,
    value: max,
  };
}

/** `#RRGGBB` of a projection colour. */
export function hexColor(color: ProjectionColor): string {
  return `#${linearColor(color)
    .map((channel) =>
      Math.round(encode(channel) * 255)
        .toString(16)
        .padStart(2, "0")
        .toUpperCase(),
    )
    .join("")}`;
}

/**
 * The tuning's setters and the operator events they log. The tuning itself is
 * the `tuning` field of the page's state, which components select from.
 */
export function useTuning(
  state: Store<{ readonly tuning: Tuning }>,
  record: (message: string) => void,
) {
  return useMemo(() => {
    const current = () => state.current.tuning;
    /** Change settings; a change to what a preset fixes forgets the preset. */
    const update = (change: Partial<Tuning>) => {
      const previous = current();
      const next = { ...previous, ...change };
      if (change.preset === undefined && previous.preset !== undefined) {
        const values = PRESETS.find(
          ({ key }) => key === previous.preset,
        )?.values;
        const kept =
          values !== undefined &&
          (Object.keys(values) as (keyof PresetValues)[]).every(
            (key) => JSON.stringify(values[key]) === JSON.stringify(next[key]),
          );
        if (!kept) {
          state.update({ tuning: { ...next, preset: undefined } });
          return;
        }
      }
      state.update({ tuning: next });
    };
    return {
      reset: (color: ProjectionColor) =>
        state.update({ tuning: initialTuning(color) }),
      setBeam: (beam: number) => update({ beam }),
      setLight: (light: number) => update({ light }),
      setOffset: (offset: number) => update({ offset }),
      setSweep: (sweep: readonly [number, number]) => update({ sweep }),
      setRate: (rate: ScanRate) => {
        if (rate === current().rate) return;
        update({ rate });
        record(`SCAN RATE ${rate.toUpperCase()}`);
      },
      setChannels: (channels: readonly Channel[]) => {
        update({
          // In the channels' own order, whatever order they arrive in.
          channels: CHANNELS.map(({ key }) => key).filter((key) =>
            channels.includes(key),
          ),
        });
        record(
          `CHANNELS ${channels.length === 0 ? "NONE" : channels.map((key) => key.toUpperCase()).join(" ")}`,
        );
      },
      setPreset: (key: string) => {
        const preset = PRESETS.find((entry) => entry.key === key);
        if (!preset) return;
        update({ ...preset.values, preset: key });
        record(`PRESET ${preset.label}`);
      },
      setGrid: (grid: ScopeGrid) => {
        if (grid === current().grid) return;
        update({ grid });
        record(`SCOPE GRID ${grid.toUpperCase()}`);
      },
      setSweepShown: (sweepShown: boolean) => {
        if (sweepShown === current().sweepShown) return;
        update({ sweepShown });
        record(`SCOPE SWEEP ${sweepShown ? "ON" : "OFF"}`);
      },
      setFocus: (focus: string | undefined, label?: string) => {
        if (focus === current().focus) return;
        update({ focus });
        if (label) record(`FOCUS ${label}`);
      },
      setColor: (color: ProjectionColor) => update({ color }),
    };
  }, [state, record]);
}

export type TuningActions = ReturnType<typeof useTuning>;
