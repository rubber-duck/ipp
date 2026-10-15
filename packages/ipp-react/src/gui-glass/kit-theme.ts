import { glassNativeParts } from "./native-parts.js";
import { glassBaseTokens } from "./tokens.js";
import { glassControlMetrics } from "./metrics.js";
import type { GuiKitRecipeInputs } from "../gui-kit/themes.js";
import type { GuiKitContract, GuiKitTokens } from "../gui-kit/kit.js";
import type { KitTheme } from "../gui-kit/themes.js";
import { extendGlassLook, glassFocusRows, glassTextRows } from "./rows.js";
import type {
  GlassColor,
  GlassContract,
  GlassRecipeInputs,
  GlassPalette,
  GlassRow,
  GlassTheme,
} from "./types.js";

export type GlassKitContract = GlassContract & GuiKitContract;
export type GlassKitRecipes = GlassKitContract & GuiKitRecipeInputs;

export interface GlassKitTheme extends GlassTheme {
  readonly accent?: GlassColor;
  readonly body?: readonly [number, number, number, number];
}

export interface GlassKitThemeOptions {
  readonly parts?: readonly string[];
  readonly accent?: GlassColor;
  readonly body?: readonly [number, number, number, number];
  readonly rows?: readonly GlassRow[];
}

/** Native coverage supplies the silhouette; painted boxes own their optical rim. */
export function glassKitTheme(
  contract: GlassKitRecipes,
  palette: GlassPalette,
  base: KitTheme,
  options: GlassKitThemeOptions = {},
): GlassKitTheme {
  const rows = (options.parts ?? ["background"]).flatMap((part) => {
    const index = contract.guiPaintPartIndex({ part });
    const shape = base.rows.find((row) => row.part === index)?.shape ?? 0;
    return [undefined, "checked", "unchecked"].flatMap((variant) =>
      (variant
        ? ["idle", "hovered", "pressed", "disabled"]
        : [undefined, "idle", "hovered", "pressed", "disabled"]
      ).map((state) => ({
        part: contract.guiPaintPartIndex({
          part,
          ...(state ? { state } : {}),
          ...(variant ? { variant } : {}),
        }),
        paint: true,
        paint_data: [
          state === "hovered" || state === "pressed" ? 1 : 0,
          state === "pressed" ? 1 : 0,
          state === "disabled" ? 1 : 0,
          variant === "checked" ? 1 : 0,
        ],
        color: palette.surface,
        fill_mode: 0,
        opacity: 1,
        corner_cut: [0, 0, 0, 0],
        corner_radius: [0, 0],
        // Box borders cover the shader's rim; arc/stroke widths define their
        // geometry and must survive. Native glow needs no blur or extra pass.
        ...(shape === 0 ? { border_width: 0, corner_accent_width: 0 } : {}),
        glow_color: options.accent ?? palette.primary,
        glow_intensity: 0,
        glow_radius: 0,
        glow_inner_radius: 0,
      })),
    );
  });
  return {
    ...extendGlassLook(
      {
        parts: base.rows,
        em: base.em,
        metrics: base.metrics,
        motion: base.motion,
      },
      [
        ...rows,
        ...glassTextRows(contract, palette),
        ...glassFocusRows(contract, palette),
        ...(options.rows ?? []),
      ],
    ),
    accent: options.accent ?? palette.primary,
    body: options.body ?? [6, 0, 1, 0],
  };
}

export function glassKitTokens(p: GlassPalette): GuiKitTokens {
  return {
    ...glassBaseTokens,
    page: p.background,
    surface: p.surface,
    accent: p.primary,
    text: p.text,
    neutral: p.neutral,
    line: p.secondary,
    amber: p.warning,
    error: p.error,
    rowTint: [p.primary[0], p.primary[1], p.primary[2], 0.04],
    focusGlowIntensity: 0,
    frameGlowReach: 0,
  };
}

/** Recolour the explicit Glass base rows by semantic role before deriving composite recipes. */
export function glassKitContract<T extends GlassContract>(
  contract: T,
  p: GlassPalette,
  overrides: Partial<GuiKitTokens> = {},
): T & GlassRecipeInputs & Pick<GuiKitRecipeInputs, "tokens" | "looks"> {
  const tokens = { ...glassKitTokens(p), ...overrides };
  const roles = [
    "page",
    "surface",
    "accent",
    "text",
    "neutral",
    "line",
    "amber",
    "error",
    "rowTint",
  ] as const;
  const recolor = (row: GlassRow): GlassRow =>
    Object.fromEntries(
      Object.entries(row).map(([name, value]) => {
        if (
          (name === "color" || name.includes("color")) &&
          Array.isArray(value) &&
          value.length === 4
        ) {
          const role = roles.find((role) =>
            glassBaseTokens[role].slice(0, 3).every((v, i) => v === value[i]),
          );
          if (role) return [name, [...tokens[role].slice(0, 3), value[3]]];
        }
        if (name === "corner_cut") return [name, [0, 0, 0, 0]];
        if (
          name === "glow_intensity" ||
          name === "glow_radius" ||
          name === "glow_inner_radius" ||
          name === "corner_accent_width"
        )
          return [name, 0];
        return [name, value];
      }),
    ) as GlassRow;
  return {
    ...contract,
    tokens,
    looks: Object.fromEntries(
      Object.entries(glassNativeParts(contract)).map(([name, look]) => [
        name,
        {
          ...look,
          parts: look.parts.map(recolor),
          metrics: glassControlMetrics(name),
        },
      ]),
    ) as unknown as GlassKitRecipes["looks"],
  };
}
