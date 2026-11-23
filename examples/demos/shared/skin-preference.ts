/** The shared application owns this choice; the runtime stores no skin names. */
export const DEMO_SKINS = [
  { key: "glass", label: "Glass" },
  { key: "cybertron", label: "Cybertron" },
  { key: "cartoon", label: "Cartoon" },
] as const;

export type GallerySkin = (typeof DEMO_SKINS)[number]["key"];

export function isGallerySkin(value: unknown): value is GallerySkin {
  return DEMO_SKINS.some(({ key }) => key === value);
}
