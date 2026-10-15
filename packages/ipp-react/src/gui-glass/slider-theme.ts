import { extendGlassLook, glassFocusRows, glassStateRows } from "./rows.js";
import type { GlassRecipeInputs, GlassPalette, GlassTheme } from "./types.js";

export function sliderTheme(
  contract: GlassRecipeInputs,
  palette: GlassPalette,
): GlassTheme {
  const part = (name: string, role: number) =>
    glassStateRows(contract, palette, name).map((row) => ({
      ...row,
      paint_data: [...(row.paint_data as number[]).slice(0, 3), role],
    }));
  return extendGlassLook(contract.looks.slider!, [
    ...part("background", 0),
    ...part("fill", 1),
    ...part("icon", 2),
    ...glassFocusRows(contract, palette),
  ]);
}
