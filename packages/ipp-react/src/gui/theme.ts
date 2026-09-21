/** Runtime-authored GUI themes.
 *
 * React only declares named-part lanes on GuiRoot. Core chooses interaction
 * state and checked/focus variants, and rendering consumes the resolved lanes.
 * No browser/TypeScript interaction resolver exists here.
 */
import {
  guiPartProperty,
  type DynamicValue,
  type GuiAssetSource,
  type GuiPartProperty,
} from "@ipp/client";

export const GUI_THEME_PARTS = [
  "background",
  "fill",
  "label",
  "icon",
  "focusRing",
] as const;

export type GuiThemePartName = (typeof GUI_THEME_PARTS)[number];
export type GuiThemeState = "idle" | "hovered" | "pressed" | "disabled";
export type GuiThemeVariant = "checked" | "unchecked";

/** One authored runtime part. Checked/unchecked lanes are written for every
 * interaction state so core can apply its normal candidate precedence. */
export interface GuiThemeLaneStyle {
  readonly color?: readonly [number, number, number, number] | undefined;
  readonly opacity?: number | undefined;
  readonly scale?: readonly [number, number] | undefined;
  readonly asset?: GuiAssetSource | undefined;
  /** Optional state-qualified transition into this appearance. */
  readonly transition?: GuiThemeTransition | undefined;
}

/** Animation clip configuration for core-owned appearance transitions. */
export interface GuiThemeTransition {
  readonly motion: GuiAssetSource;
  readonly duration: number;
  readonly easing?: "linear" | "smoothstep" | undefined;
  readonly track?: number | undefined;
  readonly time?: number | undefined;
}

export interface GuiThemedPart {
  readonly base?: GuiThemeLaneStyle | undefined;
  readonly idle?: GuiThemeLaneStyle | undefined;
  readonly hovered?: GuiThemeLaneStyle | undefined;
  readonly pressed?: GuiThemeLaneStyle | undefined;
  readonly disabled?: GuiThemeLaneStyle | undefined;
  readonly checked?: GuiThemeLaneStyle | undefined;
  readonly unchecked?: GuiThemeLaneStyle | undefined;
}

/** App-authored lanes for the runtime's stable primitive parts. */
export interface GuiControlTheme {
  /** Font measured by layout before label glyphs are painted. An explicit
   * node style asset overrides this theme default. */
  readonly font?: GuiAssetSource | undefined;
  readonly parts: Readonly<
    Partial<Record<GuiThemePartName, GuiThemedPart | undefined>>
  >;
}

const states: readonly GuiThemeState[] = [
  "idle",
  "hovered",
  "pressed",
  "disabled",
];
const variants: readonly GuiThemeVariant[] = ["checked", "unchecked"];
const partKeys = new Set<string>(GUI_THEME_PARTS);
const themedPartKeys = new Set(["base", ...states, ...variants]);

function inUnit(value: number): boolean {
  return Number.isFinite(value) && value >= 0 && value <= 1;
}

function validateAsset(value: GuiAssetSource, what: string): void {
  if (
    typeof value !== "object" ||
    value === null ||
    !Number.isInteger(value.kind) ||
    value.kind <= 0 ||
    value.kind > 65535 ||
    typeof value.source !== "string" ||
    value.source.length === 0 ||
    (value.variant !== undefined &&
      (!Number.isInteger(value.variant) ||
        value.variant < 0 ||
        value.variant > 0xffffffff))
  )
    throw new Error(`GUI theme ${what} must be a valid asset source`);
}

export function validateThemeLaneStyle(
  style: GuiThemeLaneStyle,
  what: string,
): void {
  if (typeof style !== "object" || style === null)
    throw new Error(`GUI theme ${what} must be an object`);
  for (const key of Object.keys(style))
    if (!["color", "opacity", "scale", "asset", "transition"].includes(key))
      throw new Error(`GUI theme ${what} has unknown lane "${key}"`);
  if (
    style.color !== undefined &&
    (!Array.isArray(style.color) ||
      style.color.length !== 4 ||
      !style.color.every((entry) => typeof entry === "number" && inUnit(entry)))
  )
    throw new Error(
      `GUI theme ${what} color must be four finite numbers in 0..1`,
    );
  if (
    style.opacity !== undefined &&
    (typeof style.opacity !== "number" || !inUnit(style.opacity))
  )
    throw new Error(
      `GUI theme ${what} opacity must be a finite number in 0..1`,
    );
  if (
    style.scale !== undefined &&
    (!Array.isArray(style.scale) ||
      style.scale.length !== 2 ||
      !style.scale.every(
        (entry) => typeof entry === "number" && Number.isFinite(entry),
      ))
  )
    throw new Error(`GUI theme ${what} scale must be two finite numbers`);
  if (style.asset !== undefined) validateAsset(style.asset, `${what}.asset`);
  if (style.transition !== undefined)
    validateTransition(style.transition, `${what}.transition`);
}

function validateTransition(
  transition: GuiThemeTransition,
  what: string,
): void {
  if (typeof transition !== "object" || transition === null)
    throw new Error(`GUI theme ${what} must be an object`);
  for (const key of Object.keys(transition))
    if (!["motion", "duration", "easing", "track", "time"].includes(key))
      throw new Error(`GUI theme ${what} has unknown key "${key}"`);
  validateAsset(transition.motion, `${what}.motion`);
  if (!Number.isFinite(transition.duration) || transition.duration < 0)
    throw new Error(
      `GUI theme ${what}.duration must be finite and nonnegative`,
    );
  if (
    transition.easing !== undefined &&
    transition.easing !== "linear" &&
    transition.easing !== "smoothstep"
  )
    throw new Error(`GUI theme ${what}.easing is invalid`);
  if (
    transition.track !== undefined &&
    (!Number.isInteger(transition.track) ||
      transition.track < 0 ||
      transition.track > 0xffff_fffd)
  )
    throw new Error(
      `GUI theme ${what}.track must be an integer in 0..=u32::MAX-2`,
    );
  if (
    transition.track !== undefined &&
    Math.fround(transition.track) !== transition.track
  )
    throw new Error(
      `GUI theme ${what}.track must be represented exactly as f32`,
    );
  if (
    transition.time !== undefined &&
    (!Number.isFinite(transition.time) || transition.time < 0)
  )
    throw new Error(`GUI theme ${what}.time must be finite and nonnegative`);
}

/** Validate only declarations the runtime can consume. */
export function validateGuiTheme(theme: GuiControlTheme): void {
  if (typeof theme !== "object" || theme === null)
    throw new Error("GUI theme must be an object");
  if (typeof theme.parts !== "object" || theme.parts === null)
    throw new Error("GUI theme parts must be an object");
  for (const key of Object.keys(theme))
    if (key !== "font" && key !== "parts")
      throw new Error(`GUI theme has unknown key "${key}"`);
  if (theme.font !== undefined) validateAsset(theme.font, "font");
  for (const [name, part] of Object.entries(theme.parts)) {
    if (!partKeys.has(name))
      throw new Error(`GUI theme part "${name}" is not a runtime named part`);
    if (part === undefined) continue;
    if (typeof part !== "object" || part === null)
      throw new Error(`GUI theme part "${name}" must be an object`);
    for (const key of Object.keys(part))
      if (!themedPartKeys.has(key))
        throw new Error(`GUI theme part "${name}" has unknown key "${key}"`);
    for (const key of ["base", ...states, ...variants] as const) {
      const lanes = part[key];
      if (lanes === undefined) continue;
      validateThemeLaneStyle(lanes, `${name}.${key}`);
      if (name === "label" && lanes.asset !== undefined)
        throw new Error(
          "GUI theme label.asset is unsupported; use theme.font so layout measures the selected font",
        );
    }
    const animated = [
      part.base,
      ...states.map((state) => part[state]),
      ...variants.map((variant) => part[variant]),
    ].some((lanes) => lanes?.transition !== undefined);
    if (
      animated &&
      (part.base?.color === undefined ||
        part.base.opacity === undefined ||
        part.base.scale === undefined)
    )
      throw new Error(
        `GUI theme animated part "${name}" requires base color, opacity, and scale lanes`,
      );
  }
}

function dynamicLanes(
  result: Record<string, DynamicValue>,
  node: number,
  part: string,
  lanes: GuiThemeLaneStyle,
): void {
  const set = (lane: GuiPartProperty, value: DynamicValue): void => {
    result[guiPartProperty(node, part, lane)] = value;
  };
  if (lanes.color !== undefined)
    set("color", { kind: "vec4", value: [...lanes.color] });
  if (lanes.opacity !== undefined)
    set("opacity", { kind: "f32", value: lanes.opacity });
  if (lanes.scale !== undefined)
    set("scale", { kind: "vec2", value: [...lanes.scale] });
  if (lanes.asset !== undefined)
    set("asset", { kind: "asset", value: { ...lanes.asset } });
  const transition = lanes.transition;
  if (transition !== undefined) {
    const prefix = `node_${node}_part_${part}`;
    result[`${prefix}_motion`] = {
      kind: "asset",
      value: { ...transition.motion },
    };
    result[`${prefix}_duration`] = {
      kind: "f32",
      value: transition.duration,
    };
    result[`${prefix}_easing`] = {
      kind: "f32",
      value: transition.easing === "smoothstep" ? 1 : 0,
    };
    result[`${prefix}_track`] = {
      kind: "f32",
      value: transition.track ?? 0,
    };
    result[`${prefix}_time`] = {
      kind: "f32",
      value: transition.time ?? 0,
    };
  }
}

/** Compile one theme to the ordinary GuiRoot named-property namespace. */
export function guiThemeProperties(
  node: number,
  theme: GuiControlTheme | undefined,
): Readonly<Record<string, DynamicValue>> {
  if (theme === undefined) return {};
  validateGuiTheme(theme);
  const result: Record<string, DynamicValue> = {};
  for (const partName of GUI_THEME_PARTS) {
    const part = theme.parts[partName];
    if (part === undefined) continue;
    if (part.base !== undefined)
      dynamicLanes(result, node, partName, part.base);
    for (const state of states) {
      const lanes = part[state];
      if (lanes !== undefined)
        dynamicLanes(result, node, `${partName}_${state}`, lanes);
    }
    for (const variant of variants) {
      const lanes = part[variant];
      if (lanes === undefined) continue;
      for (const state of states)
        dynamicLanes(result, node, `${partName}_${state}_${variant}`, lanes);
    }
  }
  return result;
}

/** Apply the theme's measured font default without overriding an explicit
 * node asset (including an explicit null clear). Named paint parts remain
 * ordinary theme properties: core materializes control geometry from them,
 * so React never duplicates background colors into node style. */
export function guiStyleWithTheme<T extends { asset?: GuiAssetSource | null }>(
  style: T,
  theme: GuiControlTheme | undefined,
): T {
  if (theme?.font === undefined || style.asset !== undefined) return style;
  return { ...style, asset: { ...theme.font } };
}

/** Generic defaults authored through the same runtime part namespace. */
export const defaultGuiTheme: GuiControlTheme = {
  parts: {
    background: {
      base: { color: [0.16, 0.34, 0.72, 1], opacity: 1, scale: [1, 1] },
      hovered: { color: [0.2, 0.4, 0.85, 1] },
      pressed: { color: [0.1, 0.25, 0.6, 1] },
      disabled: { opacity: 0.4 },
      checked: { color: [0.16, 0.55, 0.3, 1] },
    },
    label: { base: { color: [1, 1, 1, 1] } },
    icon: {
      checked: { color: [1, 1, 1, 1], opacity: 1 },
      unchecked: { color: [1, 1, 1, 1], opacity: 0 },
    },
    focusRing: { base: { color: [1, 1, 1, 1], opacity: 1 } },
  },
};
