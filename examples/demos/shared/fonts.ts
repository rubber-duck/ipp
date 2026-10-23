/** Application text choices; icons and terminal cells retain the Nerd Font. */
export const DEMO_FONTS = [
  {
    key: "shure-tech-mono",
    label: "Shure Tech Mono",
    path: "/target/font-assets/shure-tech-mono.ippf",
  },
  {
    key: "noto-sans",
    label: "Noto Sans",
    path: "/target/font-assets/noto-sans.ippf",
  },
] as const;

export type GalleryFont = (typeof DEMO_FONTS)[number]["key"];
export const DEFAULT_DEMO_FONT: GalleryFont = "shure-tech-mono";
export const DEMO_ICON_FONT = DEMO_FONTS[0].path;
export const DEMO_FONT_RESOURCES = DEMO_FONTS.map(({ path }) => ({
  kind: 17,
  path,
}));

export function demoFont(value: unknown = DEFAULT_DEMO_FONT) {
  const font = DEMO_FONTS.find(({ key }) => key === value);
  if (!font) throw new Error(`Unknown demo font: ${String(value)}`);
  return font;
}
