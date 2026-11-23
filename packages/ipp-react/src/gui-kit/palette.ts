import type { GuiKitColor } from "./kit.js";

/** Decode a six/eight-digit sRGB hex colour exactly once at the authoring boundary. */
export function paletteColor(hex: string): GuiKitColor {
  if (!/^#[\da-f]{6}([\da-f]{2})?$/i.test(hex))
    throw new TypeError(`Palette colours must be #RRGGBB or #RRGGBBAA: ${hex}`);
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
