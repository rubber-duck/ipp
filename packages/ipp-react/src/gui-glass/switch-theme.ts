import { compactControlRows } from "../gui-kit/sizing.js";
import {
  extendGlassLook,
  glassFocusRows,
  glassStateRows,
  glassTextRows,
} from "./rows.js";
import type { GlassRecipeInputs, GlassPalette, GlassTheme } from "./types.js";

export function switchTheme(
  contract: GlassRecipeInputs,
  palette: GlassPalette,
): GlassTheme {
  return extendGlassLook(contract.looks.switch!, [
    ...glassStateRows(contract, palette),
    ...glassTextRows(contract, palette),
    ...glassStateRows(contract, palette, "icon", { scale: [1, 1] }).map(
      (row) => ({
        ...row,
        paint_data: [...(row.paint_data as number[]).slice(0, 3), 2],
      }),
    ),
    ...glassFocusRows(contract, palette),
    ...compactControlRows(contract, "switch", [], "shader"),
  ]);
}
