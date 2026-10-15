import type { GuiKitLook } from "../gui-kit/kit.js";
import { glassAlpha } from "./palette.js";
import type {
  GlassContract,
  GlassPalette,
  GlassRow,
  GlassTheme,
} from "./types.js";

export function extendGlassLook(
  look: GuiKitLook & { readonly motion?: readonly GlassRow[] | undefined },
  rows: readonly GlassRow[],
): GlassTheme {
  const merged = new Map(look.parts.map((row) => [row.part, row]));
  for (const row of rows)
    merged.set(row.part, { ...merged.get(row.part), ...row });
  return {
    rows: [...merged.values()],
    em: look.em,
    metrics: look.metrics,
    ...(look.motion ? { motion: look.motion } : {}),
  };
}

/** Native state lanes: activity, press, disabled, selection. Overrides apply to every row. */
export function glassStateRows(
  contract: GlassContract,
  palette: GlassPalette,
  part = "background",
  overrides: Omit<GlassRow, "part"> = {},
): readonly GlassRow[] {
  return [undefined, "checked", "unchecked"].flatMap((variant) =>
    ["idle", "hovered", "pressed", "disabled"].map((state) => ({
      part: contract.guiPaintPartIndex({
        part,
        state,
        ...(variant ? { variant } : {}),
      }),
      color: glassAlpha(palette.surface, 1),
      paint: true,
      paint_data: [
        state === "hovered" || state === "pressed" ? 1 : 0,
        state === "pressed" ? 1 : 0,
        state === "disabled" ? 1 : 0,
        variant === "checked" ? 1 : 0,
      ],
      outset: [0, 0],
      opacity: 1,
      scale: [1, 1],
      border_width: 0,
      glow_intensity: 0,
      corner_cut: [0, 0, 0, 0],
      corner_radius: [0, 0],
      corner_accent_width: 0,
      ...overrides,
    })),
  );
}

/** Text/placeholder share native Label geometry; caret and selection belong to input themes. */
export function glassTextRows(
  contract: GlassContract,
  palette: GlassPalette,
): readonly GlassRow[] {
  return [undefined, "checked", "unchecked"].flatMap((variant) =>
    ["idle", "hovered", "pressed", "disabled"].map((state) => ({
      part: contract.guiPaintPartIndex({
        part: "label",
        state,
        ...(variant ? { variant } : {}),
      }),
      color: state === "disabled" ? palette.mutedText : palette.text,
      paint: false,
      opacity: 1,
    })),
  );
}

export function glassFocusRows(
  contract: GlassContract,
  palette: GlassPalette,
): readonly GlassRow[] {
  return [
    {
      part: contract.guiPaintPartIndex({ part: "focusRing" }),
      paint: true,
      paint_data: [0, 0, 0, 4],
      color: palette.text,
      border_width: 0,
      corner_cut: [0, 0, 0, 0],
      corner_radius: [0, 0],
      scale: [1, 1],
      outset: [0, 0],
      glow_intensity: 0,
      opacity: 1,
    },
  ];
}
