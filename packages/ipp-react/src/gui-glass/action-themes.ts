import {
  extendGlassLook,
  glassFocusRows,
  glassStateRows,
  glassTextRows,
} from "./rows.js";
import { glassAlpha } from "./palette.js";
import type { GlassRecipeInputs, GlassPalette, GlassTheme } from "./types.js";

/** Native controls retain editing, geometry and motion; all decoration is optical. */
export function actionThemes(
  contract: GlassRecipeInputs,
  palette: GlassPalette,
): { button: GlassTheme; textInput: GlassTheme } {
  const text = glassTextRows(contract, palette),
    focus = glassFocusRows(contract, palette);
  const key = (part: string) => contract.guiPaintPartIndex({ part });
  return {
    button: extendGlassLook(contract.looks.button!, [
      ...glassStateRows(contract, palette),
      ...text,
      ...focus,
    ]),
    textInput: extendGlassLook(contract.looks.textInput!, [
      ...glassStateRows(contract, palette).map((row) => ({
        ...row,
        paint_data: [...(row.paint_data as number[]).slice(0, 3), -1],
      })),
      ...text,
      ...focus,
      { part: key("caret"), color: palette.primary, paint: false },
      {
        part: key("selection"),
        color: glassAlpha(palette.primary, 0.24),
        paint: false,
      },
      ...["decrement", "increment"].flatMap((part, i) =>
        glassStateRows(contract, palette, part, { shape: 0 }).map((row) => ({
          ...row,
          paint_data: [...(row.paint_data as number[]).slice(0, 3), -3 - i],
        })),
      ),
      ...["decrementMark", "incrementMark"].flatMap((part) =>
        ["idle", "hovered", "pressed", "disabled"].map((state) => ({
          part: contract.guiPaintPartIndex({ part, state }),
          paint: false,
          color: state === "disabled" ? palette.mutedText : palette.text,
          glow_intensity: 0,
        })),
      ),
    ]),
  };
}
