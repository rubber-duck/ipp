import { checkboxTheme } from "./checkbox-theme.js";
import { switchTheme } from "./switch-theme.js";
import { sliderTheme } from "./slider-theme.js";
import type { GlassRecipeInputs, GlassPalette, GlassTheme } from "./types.js";

/** Native geometry and states remain owned by the receiving Host. */
export function valueThemes(
  contract: GlassRecipeInputs,
  palette: GlassPalette,
): { checkbox: GlassTheme; switch: GlassTheme; slider: GlassTheme } {
  return {
    checkbox: checkboxTheme(contract, palette),
    switch: switchTheme(contract, palette),
    slider: sliderTheme(contract, palette),
  };
}
