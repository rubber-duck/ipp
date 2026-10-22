export const COCKPIT_SCREEN_TITLES = [
  "Bridge controls",
  "System monitor",
  "Terminal",
  "Browser",
] as const;
export const COCKPIT_PANE_COUNT = COCKPIT_SCREEN_TITLES.length;

/** Operator settings; component state and animation clocks remain in the World. */
export interface CockpitOptions {
  charge: number;
  panelOpacity: number;
  skin: "glass" | "cybertron";
  projection: number;
  surface: "flat" | "curved";
  exploded: boolean;
  surfaceCaching: "off" | "auto" | "force";
  surfaceRefreshHz: number;
  surfaceTextureSize: number;
  carousel: number;
  carouselDragging: boolean;
  flightMode: string;
  autopilot: boolean;
  shields: boolean;
  power: number;
  channel: string;
  message: string;
  muted: boolean;
  gain: number;
  range: number;
  scanning: boolean;
  callsign: string;
  pulse: number;
  atmosphere: number;
  atmospherePlaying: boolean;
  telemetrySmooth: boolean;
  illumination: number;
  lightsPlaying: boolean;
  cities: number;
  stars: number;
}

// The 4 m sheets extend 1.35 m below the camera. At that height the
// projected console reaches a radius of 8.60 m; retain 20 cm of clearance.
export const COCKPIT_SCREEN_DISTANCE = { min: 8.8, max: 12 } as const;

export const COCKPIT_DEFAULTS: CockpitOptions = {
  charge: 0.7,
  panelOpacity: 0.98,
  skin: "glass",
  projection: 10,
  surface: "curved",
  exploded: false,
  surfaceCaching: "off",
  surfaceRefreshHz: 5,
  surfaceTextureSize: 2048,
  carousel: 0,
  carouselDragging: false,
  flightMode: "cruise",
  autopilot: true,
  shields: true,
  power: 0.72,
  channel: "bridge",
  message: "Request docking clearance",
  muted: false,
  gain: 0.64,
  range: 3.2,
  scanning: true,
  callsign: "IPP EXPLORER",
  pulse: 0,
  atmosphere: 1,
  atmospherePlaying: true,
  telemetrySmooth: false,
  illumination: 1,
  lightsPlaying: true,
  cities: 1,
  stars: 1,
};

export function cockpitOptions(
  value: Readonly<Record<string, unknown>>,
): CockpitOptions {
  const number = (
    name: keyof CockpitOptions,
    minimum: number,
    maximum: number,
  ) => {
    const result = Number(value[name] ?? COCKPIT_DEFAULTS[name]);
    if (!Number.isFinite(result)) throw new Error(`Invalid cockpit ${name}`);
    return Math.max(minimum, Math.min(maximum, result));
  };
  return {
    charge: number("charge", 0, 1),
    panelOpacity: number("panelOpacity", 0.55, 1),
    skin: value.skin === "cybertron" ? "cybertron" : "glass",
    surface: value.surface === "flat" ? "flat" : "curved",
    exploded: Boolean(value.exploded),
    surfaceCaching:
      value.surfaceCaching === "auto" || value.surfaceCaching === "force"
        ? value.surfaceCaching
        : "off",
    surfaceRefreshHz: number("surfaceRefreshHz", 0.5, 30),
    surfaceTextureSize: Math.round(number("surfaceTextureSize", 256, 2048)),
    projection: number(
      "projection",
      COCKPIT_SCREEN_DISTANCE.min,
      COCKPIT_SCREEN_DISTANCE.max,
    ),
    carousel: number("carousel", 0, COCKPIT_PANE_COUNT - 1),
    carouselDragging: Boolean(value.carouselDragging),
    flightMode: String(value.flightMode ?? COCKPIT_DEFAULTS.flightMode),
    autopilot: value.autopilot === undefined ? true : Boolean(value.autopilot),
    shields: value.shields === undefined ? true : Boolean(value.shields),
    power: number("power", 0, 1),
    channel: String(value.channel ?? COCKPIT_DEFAULTS.channel),
    message: String(value.message ?? COCKPIT_DEFAULTS.message),
    muted: Boolean(value.muted),
    gain: number("gain", 0, 1),
    range: number("range", 1.2, 4),
    scanning: value.scanning === undefined ? true : Boolean(value.scanning),
    callsign: String(value.callsign ?? COCKPIT_DEFAULTS.callsign),
    pulse: Math.round(number("pulse", 0, Number.MAX_SAFE_INTEGER)),
    atmosphere: number("atmosphere", 0, 1.5),
    atmospherePlaying:
      value.atmospherePlaying === undefined
        ? true
        : Boolean(value.atmospherePlaying),
    telemetrySmooth: Boolean(value.telemetrySmooth),
    illumination: number("illumination", 0, 1.5),
    lightsPlaying:
      value.lightsPlaying === undefined ? true : Boolean(value.lightsPlaying),
    cities: number("cities", 0, 1.5),
    stars: number("stars", 0, 1.5),
  };
}
