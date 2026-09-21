/** Color inputs use sRGB; authored unlit colors use linear RGB. */
export function hexToLinear(value: string): [number, number, number] {
  const channel = (offset: number) => {
    const encoded = Number.parseInt(value.slice(offset, offset + 2), 16) / 255;
    return encoded <= 0.04045
      ? encoded / 12.92
      : ((encoded + 0.055) / 1.055) ** 2.4;
  };
  return [channel(1), channel(3), channel(5)];
}

export function linearToHex(color: readonly [number, number, number]): string {
  return `#${color
    .map((value) => {
      const encoded =
        value <= 0.0031308 ? 12.92 * value : 1.055 * value ** (1 / 2.4) - 0.055;
      return Math.round(Math.max(0, Math.min(1, encoded)) * 255)
        .toString(16)
        .padStart(2, "0");
    })
    .join("")}`;
}
