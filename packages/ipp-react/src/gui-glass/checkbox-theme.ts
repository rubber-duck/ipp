import { compactControlRows } from "../gui-kit/sizing.js";
import {
  extendGlassLook,
  glassFocusRows,
  glassStateRows,
  glassTextRows,
} from "./rows.js";
import type { GlassRecipeInputs, GlassPalette, GlassTheme } from "./types.js";

export function checkboxTheme(
  contract: GlassRecipeInputs,
  palette: GlassPalette,
): GlassTheme {
  return extendGlassLook(contract.looks.checkbox!, [
    ...glassStateRows(contract, palette),
    ...glassTextRows(contract, palette),
    ...glassFocusRows(contract, palette),
    ...[undefined, "checked", "unchecked"].flatMap((variant) =>
      (variant
        ? ["idle", "hovered", "pressed", "disabled"]
        : [undefined, "idle", "hovered", "pressed", "disabled"]
      ).map((state) => ({
        part: contract.guiPaintPartIndex({
          part: "icon",
          ...(state ? { state } : {}),
          ...(variant ? { variant } : {}),
        }),
        paint: false,
        opacity: variant === "checked" ? 1 : 0,
        color: state === "disabled" ? palette.mutedText : palette.text,
        shape: 1,
        stroke_a: [0.2, 0.48, 0.43, 0.71],
        stroke_b: [0.43, 0.71, 0.83, 0.29],
        border_width: 2,
        glow_intensity: 0,
      })),
    ),
    ...compactControlRows(contract, "checkbox", [], "shader"),
  ]);
}
