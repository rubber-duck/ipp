import { paletteColor as glassColor } from "../gui-kit/palette.js";
export { glassColor };
import type { GlassColor, GlassPalette, GlassPaletteInput } from "./types.js";

export const GLASS_PALETTE: GlassPaletteInput = Object.freeze({
  primary: "#B4E7FF",
  secondary: "#A6C1CE",
  surface: "#303942",
  background: "#19232D",
  text: "#F2F8FC",
  mutedText: "#CFDDE3",
  neutral: "#A6C1CE",
  success: "#70CBAA",
  warning: "#F2C568",
  error: "#E9848E",
});

export function resolveGlassPalette(
  input: Partial<GlassPaletteInput> = {},
): GlassPalette {
  return Object.fromEntries(
    Object.entries({ ...GLASS_PALETTE, ...input }).map(([key, color]) => [
      key,
      glassColor(color),
    ]),
  ) as unknown as GlassPalette;
}

export function glassAlpha(color: GlassColor, alpha: number): GlassColor {
  return [color[0], color[1], color[2], alpha];
}
