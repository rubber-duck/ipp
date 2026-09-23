/** Runtime-authored GUI themes.
 *
 * React compiles each distinct theme once per root into GuiRoot theme part
 * rows and points nodes at it; core chooses interaction state and
 * checked/focus variants, and rendering consumes the resolved properties.
 * No browser/TypeScript interaction resolver exists here.
 */
import {
  guiPartIndex,
  type GuiAssetSource,
  type GuiPartId,
  type GuiPartValues,
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

/** Two-stop linear or radial gradient in local shape space. Stops are
 * static material properties: transitions animate the part colour, not the
 * stops, and a missing stop takes the colour of the resolved state. */
export interface GuiThemeGradient {
  readonly kind: "linear" | "radial";
  readonly start?: readonly [number, number] | undefined;
  readonly end?: readonly [number, number] | undefined;
  readonly radius?: number | undefined;
  readonly color0?: readonly [number, number, number, number] | undefined;
  readonly color1?: readonly [number, number, number, number] | undefined;
}

/** Localized glow around the outer shape boundary. Glow properties inherit
 * from less specific parts independently of the fill; a state removes an
 * inherited glow with `intensity: 0`. */
export interface GuiThemeGlow {
  readonly color?: readonly [number, number, number, number] | undefined;
  readonly intensity?: number | undefined;
  readonly radius?: number | undefined;
  readonly falloff?: number | undefined;
}

/** One authored runtime part. Checked/unchecked styles are written for every
 * interaction state so core can apply its normal candidate precedence. */
export interface GuiThemePartStyle {
  /** Linear RGBA. Without a gradient, the part paints this colour as a
   * solid fill. When the part declares a gradient in another style, a
   * state or variant style that sets `color` without its own `gradient`
   * selects a solid fill instead of inheriting that gradient. */
  readonly color?: readonly [number, number, number, number] | undefined;
  readonly opacity?: number | undefined;
  /** Per-axis scale. A checkbox indicator scales about its own centre. */
  readonly scale?: readonly [number, number] | undefined;
  /** Horizontal checkbox-indicator position, clamped to -1..1 (default 0).
   * On a control wider than tall, -1 and +1 centre the indicator in the
   * left- and right-most height-square cells and 0 keeps it centred, so a
   * capsule checkbox reads as a switch whose knob keeps the same inset at
   * both ends. Other parts ignore it. */
  readonly alignX?: number | undefined;
  readonly asset?: GuiAssetSource | undefined;
  readonly cornerRadius?: readonly [number, number] | undefined;
  readonly borderWidth?: number | undefined;
  readonly borderColor?: readonly [number, number, number, number] | undefined;
  readonly gradient?: GuiThemeGradient | undefined;
  readonly glow?: GuiThemeGlow | undefined;
  /** Optional state-qualified transition into this appearance. */
  readonly transition?: GuiThemeTransition | undefined;
}

/** Animation clip configuration for core-owned appearance transitions.
 *
 * The clip holds color, opacity and scale tracks from `track`, sampled at
 * `time` for this state. When the part's base declares `alignX`, an
 * `align_x` track follows at `track + 3` so the indicator glides between
 * positions. Sampled values must equal the state's resolved properties. */
export interface GuiThemeTransition {
  readonly motion: GuiAssetSource;
  readonly duration: number;
  readonly easing?: "linear" | "smoothstep" | undefined;
  readonly track?: number | undefined;
  readonly time?: number | undefined;
}

export interface GuiThemedPart {
  readonly base?: GuiThemePartStyle | undefined;
  readonly idle?: GuiThemePartStyle | undefined;
  readonly hovered?: GuiThemePartStyle | undefined;
  readonly pressed?: GuiThemePartStyle | undefined;
  readonly disabled?: GuiThemePartStyle | undefined;
  readonly checked?: GuiThemePartStyle | undefined;
  readonly unchecked?: GuiThemePartStyle | undefined;
}

/** App-authored appearance for the runtime's stable primitive parts. */
export interface GuiControlTheme {
  /** Stable identity of this theme within a root. Controls whose themes
   * share a name share one runtime theme, and changing a named theme's
   * content updates it in place without touching the nodes that reference
   * it. Unnamed themes are identified by their content. */
  readonly name?: string | undefined;
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

export function validateThemePartStyle(
  style: GuiThemePartStyle,
  what: string,
): void {
  if (typeof style !== "object" || style === null)
    throw new Error(`GUI theme ${what} must be an object`);
  for (const key of Object.keys(style))
    if (
      ![
        "color",
        "opacity",
        "scale",
        "alignX",
        "asset",
        "cornerRadius",
        "borderWidth",
        "borderColor",
        "gradient",
        "glow",
        "transition",
      ].includes(key)
    )
      throw new Error(`GUI theme ${what} has unknown property "${key}"`);
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
  if (
    style.alignX !== undefined &&
    (typeof style.alignX !== "number" || !Number.isFinite(style.alignX))
  )
    throw new Error(`GUI theme ${what} alignX must be a finite number`);
  if (style.asset !== undefined) validateAsset(style.asset, `${what}.asset`);
  if (
    style.cornerRadius !== undefined &&
    (!Array.isArray(style.cornerRadius) ||
      style.cornerRadius.length !== 2 ||
      !style.cornerRadius.every(
        (entry) =>
          typeof entry === "number" && Number.isFinite(entry) && entry >= 0,
      ))
  )
    throw new Error(
      `GUI theme ${what} cornerRadius must be two non-negative finite numbers`,
    );
  if (
    style.borderWidth !== undefined &&
    (typeof style.borderWidth !== "number" ||
      !Number.isFinite(style.borderWidth) ||
      style.borderWidth < 0)
  )
    throw new Error(
      `GUI theme ${what} borderWidth must be a non-negative finite number`,
    );
  if (
    style.borderColor !== undefined &&
    (!Array.isArray(style.borderColor) ||
      style.borderColor.length !== 4 ||
      !style.borderColor.every(
        (entry) => typeof entry === "number" && inUnit(entry),
      ))
  )
    throw new Error(
      `GUI theme ${what} borderColor must be four finite numbers in 0..1`,
    );
  if (style.gradient !== undefined)
    validateGradient(style.gradient, `${what}.gradient`);
  if (style.glow !== undefined) validateGlow(style.glow, `${what}.glow`);
  if (style.transition !== undefined)
    validateTransition(style.transition, `${what}.transition`);
}

function validateGradient(gradient: GuiThemeGradient, what: string): void {
  if (typeof gradient !== "object" || gradient === null)
    throw new Error(`GUI theme ${what} must be an object`);
  for (const key of Object.keys(gradient))
    if (!["kind", "start", "end", "radius", "color0", "color1"].includes(key))
      throw new Error(`GUI theme ${what} has unknown key "${key}"`);
  if (gradient.kind !== "linear" && gradient.kind !== "radial")
    throw new Error(`GUI theme ${what}.kind must be "linear" or "radial"`);
  if (
    gradient.start !== undefined &&
    (!Array.isArray(gradient.start) ||
      gradient.start.length !== 2 ||
      !gradient.start.every(
        (entry) => typeof entry === "number" && Number.isFinite(entry),
      ))
  )
    throw new Error(`GUI theme ${what}.start must be two finite numbers`);
  if (
    gradient.end !== undefined &&
    (!Array.isArray(gradient.end) ||
      gradient.end.length !== 2 ||
      !gradient.end.every(
        (entry) => typeof entry === "number" && Number.isFinite(entry),
      ))
  )
    throw new Error(`GUI theme ${what}.end must be two finite numbers`);
  if (
    gradient.radius !== undefined &&
    (typeof gradient.radius !== "number" ||
      !Number.isFinite(gradient.radius) ||
      gradient.radius < 0)
  )
    throw new Error(
      `GUI theme ${what}.radius must be a non-negative finite number`,
    );
  if (
    gradient.color0 !== undefined &&
    (!Array.isArray(gradient.color0) ||
      gradient.color0.length !== 4 ||
      !gradient.color0.every(
        (entry) => typeof entry === "number" && inUnit(entry),
      ))
  )
    throw new Error(
      `GUI theme ${what}.color0 must be four finite numbers in 0..1`,
    );
  if (
    gradient.color1 !== undefined &&
    (!Array.isArray(gradient.color1) ||
      gradient.color1.length !== 4 ||
      !gradient.color1.every(
        (entry) => typeof entry === "number" && inUnit(entry),
      ))
  )
    throw new Error(
      `GUI theme ${what}.color1 must be four finite numbers in 0..1`,
    );
}

function validateGlow(glow: GuiThemeGlow, what: string): void {
  if (typeof glow !== "object" || glow === null)
    throw new Error(`GUI theme ${what} must be an object`);
  for (const key of Object.keys(glow))
    if (!["color", "intensity", "radius", "falloff"].includes(key))
      throw new Error(`GUI theme ${what} has unknown key "${key}"`);
  if (
    glow.color !== undefined &&
    (!Array.isArray(glow.color) ||
      glow.color.length !== 4 ||
      !glow.color.every((entry) => typeof entry === "number" && inUnit(entry)))
  )
    throw new Error(
      `GUI theme ${what}.color must be four finite numbers in 0..1`,
    );
  if (
    glow.intensity !== undefined &&
    (typeof glow.intensity !== "number" ||
      !Number.isFinite(glow.intensity) ||
      glow.intensity < 0)
  )
    throw new Error(
      `GUI theme ${what}.intensity must be a non-negative finite number`,
    );
  if (
    glow.radius !== undefined &&
    (typeof glow.radius !== "number" ||
      !Number.isFinite(glow.radius) ||
      glow.radius < 0)
  )
    throw new Error(
      `GUI theme ${what}.radius must be a non-negative finite number`,
    );
  if (
    glow.falloff !== undefined &&
    (typeof glow.falloff !== "number" ||
      !Number.isFinite(glow.falloff) ||
      glow.falloff < 0)
  )
    throw new Error(
      `GUI theme ${what}.falloff must be a non-negative finite number`,
    );
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
    if (key !== "name" && key !== "font" && key !== "parts")
      throw new Error(`GUI theme has unknown key "${key}"`);
  if (
    theme.name !== undefined &&
    (typeof theme.name !== "string" || theme.name.length === 0)
  )
    throw new Error("GUI theme name must be a nonempty string");
  if (theme.font !== undefined) validateAsset(theme.font, "font");
  for (const [name, part] of Object.entries(theme.parts)) {
    if (!partKeys.has(name))
      throw new Error(`GUI theme part "${name}" is not a runtime base part`);
    if (part === undefined) continue;
    if (typeof part !== "object" || part === null)
      throw new Error(`GUI theme part "${name}" must be an object`);
    for (const key of Object.keys(part))
      if (!themedPartKeys.has(key))
        throw new Error(`GUI theme part "${name}" has unknown key "${key}"`);
    for (const key of ["base", ...states, ...variants] as const) {
      const style = part[key];
      if (style === undefined) continue;
      validateThemePartStyle(style, `${name}.${key}`);
      if (name === "label" && style.asset !== undefined)
        throw new Error(
          "GUI theme label.asset is unsupported; use theme.font so layout measures the selected font",
        );
    }
    const animated = [
      part.base,
      ...states.map((state) => part[state]),
      ...variants.map((variant) => part[variant]),
    ].some((style) => style?.transition !== undefined);
    if (
      animated &&
      (part.base?.color === undefined ||
        part.base.opacity === undefined ||
        part.base.scale === undefined)
    )
      throw new Error(
        `GUI theme animated part "${name}" requires base color, opacity, and scale`,
      );
  }
}

/** Theme part row values for one authored part style. */
function partValues(
  style: GuiThemePartStyle,
  solidOverGradient: boolean,
): GuiPartValues {
  const values: {
    -readonly [K in keyof GuiPartValues]: GuiPartValues[K];
  } = {};
  if (style.color !== undefined) values.color = [...style.color];
  if (style.opacity !== undefined) values.opacity = style.opacity;
  if (style.scale !== undefined) values.scale = [...style.scale];
  if (style.alignX !== undefined) values.alignX = style.alignX;
  if (style.asset !== undefined) values.asset = { ...style.asset };
  if (style.cornerRadius !== undefined)
    values.cornerRadius = [...style.cornerRadius];
  if (style.borderWidth !== undefined) values.borderWidth = style.borderWidth;
  if (style.borderColor !== undefined)
    values.borderColor = [...style.borderColor];
  if (style.gradient !== undefined) {
    values.fillMode = style.gradient.kind === "radial" ? 2 : 1;
    if (style.gradient.start !== undefined)
      values.gradientStart = [...style.gradient.start];
    if (style.gradient.end !== undefined)
      values.gradientEnd = [...style.gradient.end];
    if (style.gradient.radius !== undefined)
      values.gradientRadius = style.gradient.radius;
    if (style.gradient.color0 !== undefined)
      values.gradientColor0 = [...style.gradient.color0];
    if (style.gradient.color1 !== undefined)
      values.gradientColor1 = [...style.gradient.color1];
  } else if (solidOverGradient && style.color !== undefined) {
    // Core resolves the fill mode like any other property; an explicit solid
    // mode keeps this colour from hiding under a less specific gradient.
    values.fillMode = 0;
  }
  if (style.glow !== undefined) {
    if (style.glow.color !== undefined)
      values.glowColor = [...style.glow.color];
    if (style.glow.intensity !== undefined)
      values.glowIntensity = style.glow.intensity;
    if (style.glow.radius !== undefined) values.glowRadius = style.glow.radius;
    if (style.glow.falloff !== undefined)
      values.glowFalloff = style.glow.falloff;
  }
  const transition = style.transition;
  if (transition !== undefined) {
    values.motion = { ...transition.motion };
    values.duration = transition.duration;
    values.easing = transition.easing === "smoothstep" ? 1 : 0;
    values.track = transition.track ?? 0;
    values.time = transition.time ?? 0;
  }
  return values;
}

/** Compile one theme to GuiRoot theme part rows keyed by part index
 * (see `guiPartIndex`). Variant styles apply under every state. */
export function compileGuiTheme(
  theme: GuiControlTheme,
): ReadonlyMap<number, GuiPartValues> {
  validateGuiTheme(theme);
  const rows = new Map<number, GuiPartValues>();
  const add = (id: GuiPartId, values: GuiPartValues): void => {
    if (Object.keys(values).length > 0) rows.set(guiPartIndex(id), values);
  };
  for (const partName of GUI_THEME_PARTS) {
    const part = theme.parts[partName];
    if (part === undefined) continue;
    const hasGradient = [
      part.base,
      ...states.map((state) => part[state]),
      ...variants.map((variant) => part[variant]),
    ].some((style) => style?.gradient !== undefined);
    if (part.base !== undefined)
      add({ part: partName }, partValues(part.base, false));
    for (const state of states) {
      const style = part[state];
      if (style !== undefined)
        add({ part: partName, state }, partValues(style, hasGradient));
    }
    for (const variant of variants) {
      const style = part[variant];
      if (style === undefined) continue;
      for (const state of states)
        add({ part: partName, state, variant }, partValues(style, hasGradient));
    }
  }
  return rows;
}

/** Root-local identity of a theme: its name, or else its compiled content. */
export function guiThemeKey(
  theme: GuiControlTheme,
  rows: ReadonlyMap<number, GuiPartValues> = compileGuiTheme(theme),
): string {
  if (theme.name !== undefined) return `name:${theme.name}`;
  return `content:${JSON.stringify([...rows].sort(([a], [b]) => a - b))}`;
}

/** Apply the theme's measured font default without overriding an explicit
 * node asset (including an explicit null clear). Paint parts stay in the
 * root theme: core materializes control geometry from them, so React never
 * duplicates background colors into node style. */
export function guiStyleWithTheme<T extends { asset?: GuiAssetSource | null }>(
  style: T,
  theme: GuiControlTheme | undefined,
): T {
  if (theme?.font === undefined || style.asset !== undefined) return style;
  return { ...style, asset: { ...theme.font } };
}

/** Generic defaults compiled like any other theme. */
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
