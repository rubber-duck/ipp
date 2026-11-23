import { paletteColor } from "../gui-kit/palette.js";
import { compactSizing } from "../gui-kit/sizing.js";
import type { GuiKitTokens } from "../gui-kit/kit.js";

export const ink = paletteColor("#152038");
export const cream = paletteColor("#FFF8DE");
export const gold = paletteColor("#FFCA45");
export const honey = paletteColor("#D98A24");
export const blue = paletteColor("#285682");
export const sky = paletteColor("#75DCFF");
export const leaf = paletteColor("#51A85F");
export const grass = paletteColor("#80DB85");
export const pine = paletteColor("#216754");
export const coral = paletteColor("#FF8F79");
export const clear = [0, 0, 0, 0] as const;

export const cartoonTokens = {
  ...compactSizing,
  tabPaintPadding: [4, 7],
  gridRowHeight: 32,
  gridRules: false,
  expanderFrame: true,
  progressHeight: 18,
  progressInset: 3,
  page: paletteColor("#304268"),
  surface: paletteColor("#243457"),
  accent: paletteColor("#B1EBCB"),
  text: cream,
  selectedText: ink,
  neutral: paletteColor("#B2C2D4"),
  line: paletteColor("#4A627F"),
  amber: gold,
  error: coral,
  rowTint: blue,
  lineWidth: 2,
  litLineWidth: 3,
  cut: 0,
  partCut: 0,
  cornerAccent: 0,
  cornerAccentWidth: 0,
  focusGlowIntensity: 0,
  glowFalloff: 1,
  frameGlowReach: 0,
} as const satisfies GuiKitTokens;
