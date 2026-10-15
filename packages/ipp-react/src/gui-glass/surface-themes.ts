import { glassAlpha } from "./palette.js";
import type { GlassRecipeInputs, GlassPalette, GlassTheme } from "./types.js";

/** Inert surfaces share theme-owned paint without creating input targets. */
export function surfaceThemes(
  contract: GlassRecipeInputs,
  palette: GlassPalette,
): { panel: GlassTheme; progress: GlassTheme } {
  const panel: GlassTheme = {
    em: contract.tokens.em,
    rows: [
      {
        part: contract.guiPaintPartIndex({ part: "background" }),
        color: glassAlpha(palette.surface, 1),
        paint: true,
        paint_data: [0, 0, 0, 0],
        outset: [2, 2],
      },
    ],
  };
  return {
    panel,
    progress: {
      ...panel,
      rows: panel.rows.map((row) => ({
        ...row,
        color: glassAlpha(palette.primary, 1),
        paint_data: [1, 0, 0, 1],
        outset: [1, 1],
      })),
    },
  };
}
