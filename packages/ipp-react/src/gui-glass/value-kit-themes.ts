import type { GuiKitControlName } from "../gui-kit/kit.js";
import type { KitThemeName } from "../gui-kit/themes.js";
import {
  glassKitTheme,
  type GlassKitRecipes,
  type GlassKitTheme,
} from "./kit-theme.js";
import type { GlassColor, GlassPalette, GlassRow } from "./types.js";

/** Opaque role tint, retaining each severity's hue above the dark surface. */
function bodyColor(
  surface: GlassColor,
  role: GlassColor,
  amount: number,
): GlassColor {
  const channel = (index: 0 | 1 | 2) =>
    surface[index] * (1 - amount) + role[index] * amount;
  return [channel(0), channel(1), channel(2), 1];
}

function partRows(
  contract: GlassKitRecipes,
  part: string,
  fields: Omit<GlassRow, "part">,
): GlassRow[] {
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
      ...fields,
      ...(fields.paint_data
        ? {
            paint_data: [
              state === "hovered" || state === "pressed" ? 1 : 0,
              state === "pressed" ? 1 : 0,
              state === "disabled" ? 1 : 0,
              (fields.paint_data as readonly number[])[3] ?? 0,
            ],
          }
        : {}),
    })),
  );
}

export function valueKitThemes(
  contract: GlassKitRecipes,
  palette: GlassPalette,
  base: Record<KitThemeName, GlassKitTheme>,
): Partial<Record<KitThemeName, GlassKitTheme>> {
  const background = contract.guiPaintPartIndex({ part: "background" });
  const theme = (name: KitThemeName, role: GlassColor, amount: number) =>
    glassKitTheme(contract, palette, base[name], {
      accent: role,
      rows: [
        { part: background, color: bodyColor(palette.surface, role, amount) },
      ],
    });
  const bright = (name: KitThemeName, role: GlassColor) => ({
    ...theme(name, role, 1),
    body: [8, 0, 1, 0] as const,
  });
  const ring = (name: KitThemeName, role: GlassColor, lane: number) =>
    glassKitTheme(contract, palette, base[name], {
      accent: role,
      body: [lane === 2 ? 7 : 12, 0, 1, 0],
      rows: [
        {
          part: background,
          color: lane === -1 ? bodyColor(palette.surface, role, 0.015) : role,
          paint_data: [0, 0, 0, lane],
        },
      ],
    });
  return {
    progressFrame: glassKitTheme(contract, palette, base.progressFrame, {
      body: [5, 0, 1, 0],
    }),
    valueAccent: {
      ...theme(
        "valueAccent",
        bodyColor(palette.primary, palette.secondary, 0.3),
        1,
      ),
      body: [8, 0, 1, 0],
    },
    valueNeutral: bright(
      "valueNeutral",
      bodyColor(palette.surface, palette.neutral, 0.58),
    ),
    valueAmber: bright("valueAmber", palette.warning),
    valueError: bright("valueError", palette.error),
    arcTrack: ring("arcTrack", palette.secondary, -1),
    arcAccent: ring(
      "arcAccent",
      bodyColor(palette.primary, palette.secondary, 0.3),
      1,
    ),
    arcError: ring("arcError", palette.error, 1),
    checkLit: ring("checkLit", palette.primary, 2),
    markerLit: glassKitTheme(contract, palette, base.markerLit, {
      body: [8, 0, 1, 0],
      rows: [
        {
          part: background,
          color: palette.primary,
          corner_radius: [512, 512],
          corner_cut: [0, 0, 0, 0],
        },
      ],
    }),
    // Preserve the inactive marker's hollow shape, distinct from the active mark.
    markerUnlit: {
      ...base.markerUnlit,
      rows: base.markerUnlit.rows.map((row) =>
        row.part === background
          ? {
              ...row,
              corner_radius: [512, 512],
              corner_cut: [0, 0, 0, 0],
              border_width: 0.7,
            }
          : row,
      ),
    },
    badgeAccent: {
      ...theme("badgeAccent", palette.primary, 0.015),
      body: [5, 0, 1, 0],
    },
    badgeNeutral: {
      ...theme("badgeNeutral", palette.neutral, 0.015),
      body: [5, 0, 1, 0],
    },
    badgeAmber: {
      ...theme("badgeAmber", palette.warning, 0.015),
      body: [5, 0, 1, 0],
    },
    badgeError: {
      ...theme("badgeError", palette.error, 0.015),
      body: [5, 0, 1, 0],
    },
    alertInformation: theme("alertInformation", palette.primary, 0.012),
    alertWarning: theme("alertWarning", palette.warning, 0.012),
    alertError: theme("alertError", palette.error, 0.012),
  };
}

export function valueKitControls(
  contract: GlassKitRecipes,
  palette: GlassPalette,
): Partial<Record<GuiKitControlName, GlassKitTheme>> {
  const look = (name: string): GlassKitTheme => {
    const value = contract.looks[name];
    if (!value) throw new Error(`Glass kit requires the native ${name} look`);
    return {
      rows: value.parts,
      em: value.em,
      metrics: value.metrics,
      ...(value.motion ? { motion: value.motion } : {}),
    };
  };
  return {
    dial: glassKitTheme(contract, palette, look("dial"), {
      parts: ["background", "track", "fill", "icon"],
      body: [7, 0, 1, 0],
      rows: [
        ...partRows(contract, "focusRing", {
          paint: true,
          paint_data: [0, 0, 0, 4],
          shape: 0,
          border_width: 0,
          corner_cut: [0, 0, 0, 0],
          corner_radius: [0, 0],
          scale: [1, 1],
          outset: [0, 0],
          glow_intensity: 0,
        }),
        ...partRows(contract, "background", {
          shape: 0,
          corner_radius: [512, 512],
          corner_cut: [0, 0, 0, 0],
          scale: [1, 1],
          border_width: 0,
          paint_data: [0, 0, 0, 0],
        }),
        ...partRows(contract, "ticks", {
          arc_dashes: [24, 0.06],
          border_width: 2,
          color: palette.neutral,
          paint: false,
        }),
        ...partRows(contract, "track", {
          color: bodyColor(palette.surface, palette.secondary, 0.035),
          paint_data: [0, 0, 0, -1],
          border_width: 4,
        }),
        ...partRows(contract, "fill", {
          border_width: 4,
          paint_data: [0, 0, 0, 1],
          color: bodyColor(palette.primary, palette.secondary, 0.3),
        }),
        ...partRows(contract, "icon", {
          border_width: 1.3,
          stroke_a: [0.5, 0.06, 0.5, 0.18],
          glow_color: palette.primary,
          glow_radius: 2,
          glow_intensity: 0,
          stroke_b: [0, 0, 0, 0],
          paint_data: [0, 0, 0, 2],
          color: bodyColor(palette.primary, palette.text, 0.55),
        }),
      ],
    }),
    color: glassKitTheme(contract, palette, look("color"), {
      body: [6, 0, 1, 6],
      parts: ["background"],
      rows: [
        // The large colour field has the same quiet backing as a panel. Keep
        // the exact colour area separate from optical highlights and state tints.
        ...partRows(contract, "background", {
          paint: false,
          color: palette.surface,
          corner_radius: [12, 12],
          border_width: 0,
        }),
        ...partRows(contract, "marker", {
          paint: false,
          color: [0, 0, 0, 0],
          corner_radius: [512, 512],
          corner_cut: [0, 0, 0, 0],
          border_color: palette.text,
          border_width: 0.8,
        }),
        // The runtime computes these fills from HSVA; chrome must not tint them.
        ...partRows(contract, "track", { paint: false }),
        ...partRows(contract, "fill", { paint: false }),
      ],
    }),
  };
}
