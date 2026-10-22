import type { KitThemeName } from "../gui-kit/themes.js";
import {
  glassKitTheme,
  type GlassKitRecipes,
  type GlassKitTheme,
} from "./kit-theme.js";
import type { GlassPalette } from "./types.js";
import { glassStateRows } from "./rows.js";

/** Connected choices contribute only state tint. Their parent owns all optics. */
export function selectionKitThemes(
  contract: GlassKitRecipes,
  palette: GlassPalette,
  base: Record<KitThemeName, GlassKitTheme>,
): Partial<Record<KitThemeName, GlassKitTheme>> {
  const result: Partial<Record<KitThemeName, GlassKitTheme>> = {};
  for (const name of [
    "radio",
    "stepperStep",
    "stepperComplete",
    "selectTrigger",
    "floating",
    "floatingSmall",
    "toastAccent",
    "toastAmber",
    "toastError",
  ] as const) {
    result[name] = glassKitTheme(contract, palette, base[name], {
      // The shared material owns the centered radio lens and selected dot.
      // Suppress the generic Button icon, whose label layout has a different origin.
      rows:
        name === "radio"
          ? [
              // Glass draws its centered compact contour inside the full target.
              ...glassStateRows(contract, palette, "background", {
                outset: [0, 0],
                scale: [1, 1],
              }),
              ...glassStateRows(contract, palette, "focusRing", {
                outset: [0, 0],
                scale: [1, 1],
              }),
              ...glassStateRows(contract, palette, "icon", {
                paint: false,
                opacity: 0,
              }),
            ]
          : [],
      accent:
        name === "toastError"
          ? palette.error
          : name === "toastAmber"
            ? palette.warning
            : palette.primary,
      body:
        name === "radio"
          ? [11, 0, 1, 0]
          : [
              6,
              0,
              1,
              name.startsWith("floating") || name.startsWith("toast") ? 6 : 0,
            ],
    });
  }
  for (const [name, edges] of [
    ["segmentFirst", 1],
    ["segment", 2],
    ["segmentLast", 3],
    ["segmentOnly", 4],
    ["tab", 6],
    ["menuRow", -1],
    ["menuRowAmber", -1],
    ["optionCheckRow", -1],
  ] as const)
    result[name] = glassKitTheme(contract, palette, base[name], {
      body: [9, 0, 1, edges],
      accent: name === "menuRowAmber" ? palette.warning : palette.primary,
    });
  result.tabSurface = glassKitTheme(contract, palette, base.tabSurface, {
    body: [4, 0, 1, 0],
  });
  result.tabRail = glassKitTheme(contract, palette, base.tabRail, {
    body: [9, 0, 1, 5],
  });
  return result;
}
