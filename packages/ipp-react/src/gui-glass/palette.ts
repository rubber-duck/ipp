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

/** Decode a six/eight-digit sRGB hex colour exactly once at the authoring boundary. */
export function glassColor(hex: string): GlassColor {
  if (!/^#[\da-f]{6}([\da-f]{2})?$/i.test(hex))
    throw new TypeError(
      `Glass palette colours must be #RRGGBB or #RRGGBBAA: ${hex}`,
    );
  const channel = (offset: number) => {
    const s = Number.parseInt(hex.slice(offset, offset + 2), 16) / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return [
    channel(1),
    channel(3),
    channel(5),
    hex.length === 9 ? Number.parseInt(hex.slice(7), 16) / 255 : 1,
  ];
}

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
