/** Independent pixel oracle over the maintained fixture's authored sample values. */
import type { PlotFrame } from "./plot-capture.js";

export interface ContourProbeOptions {
  origin?: readonly [number, number];
  scale?: number;
  baseline?: PlotFrame;
}

export function inspectWideSmoothContour(
  frame: PlotFrame,
  options: ContourProbeOptions = {},
) {
  const [originX, originY] = options.origin ?? [732, 178];
  const scale = options.scale ?? 1;
  let interiorPale = 0,
    rimPale = 0,
    innerRimPale = 0;
  let clearSamples = 0,
    escaped = 0,
    contrast = 0;
  const samples = 80;
  const pale = (x: number, y: number, threshold: number) => {
    x = Math.round(x);
    y = Math.round(y);
    if (x < 0 || y < 0 || x >= frame.width || y >= frame.height)
      throw new Error("Contour probe outside capture");
    const index = (y * frame.width + x) * 4;
    return [0, 1, 2].every(
      (channel) => frame.pixels[index + channel]! > threshold,
    );
  };
  // The second authored interval is x=2..4, y=45..65. Its monotone Hermite
  // derivatives are the harmonic mean of 17.5/10 at x=2, then 10 at x=4.
  // Keep away from sample labels, endpoints and the other source series.
  for (let index = 0; index < samples; index++) {
    const t = 0.12 + (0.63 * index) / (samples - 1),
      t2 = t * t,
      t3 = t2 * t;
    const m0 = 140 / 11,
      m1 = 10;
    const y =
      (2 * t3 - 3 * t2 + 1) * 45 +
      (t3 - 2 * t2 + t) * 2 * m0 +
      (-2 * t3 + 3 * t2) * 65 +
      (t3 - t2) * 2 * m1;
    const derivative =
      (6 * t2 - 6 * t) * 45 +
      (3 * t2 - 4 * t + 1) * 2 * m0 +
      (-6 * t2 + 6 * t) * 65 +
      (3 * t2 - 2 * t) * 2 * m1;
    const localX = 48 + 53.8 * (2 + 2 * t),
      localY = 275 - 2.51 * y;
    const centerX = originX + scale * localX,
      centerY = originY + scale * localY;
    if (pale(centerX, centerY, 190)) interiorPale++;
    const tangent = [107.6, -2.51 * derivative];
    const length = Math.hypot(...tangent);
    const normal = [-tangent[1]! / length, tangent[0]! / length];
    const pixel = (image: PlotFrame, x: number, y: number) => {
      const offset = (Math.round(y) * image.width + Math.round(x)) * 4;
      return [
        image.pixels[offset]!,
        image.pixels[offset + 1]!,
        image.pixels[offset + 2]!,
      ];
    };
    const center = pixel(frame, centerX, centerY);
    for (const side of [-1, 1]) {
      const probeX = centerX + side * normal[0]! * 12 * scale;
      const probeY = centerY + side * normal[1]! * 12 * scale;
      const outside = pixel(frame, probeX, probeY);
      if (options.baseline) {
        const original = pixel(options.baseline, probeX, probeY);
        // Ignore the other source series and text/grid intersections. The native
        // baseline independently identifies uncovered, quiet background pixels.
        if (Math.max(...original) < 80) {
          clearSamples++;
          if (
            outside.some(
              (value, channel) => Math.abs(value - original[channel]!) > 8,
            )
          )
            escaped++;
          contrast +=
            center.reduce(
              (sum, value, channel) =>
                sum + Math.abs(value - original[channel]!),
              0,
            ) / 3;
        }
      }
      if (
        pale(
          centerX + side * normal[0]! * 5.8 * scale,
          centerY + side * normal[1]! * 5.8 * scale,
          190,
        )
      )
        innerRimPale++;
      const x = centerX + side * normal[0]! * 6.7 * scale;
      const y = centerY + side * normal[1]! * 6.7 * scale;
      let found = false;
      // A small pixel neighbourhood admits subpixel rim phase under minification.
      for (let dy = -1; dy <= 1; dy++)
        for (let dx = -1; dx <= 1; dx++) found ||= pale(x + dx, y + dy, 145);
      if (found) rimPale++;
    }
  }
  return {
    samples,
    interiorPale,
    interiorPaleFraction: interiorPale / samples,
    rimPaleFraction: rimPale / (samples * 2),
    innerRimPaleFraction: innerRimPale / (samples * 2),
    clearSamples,
    escaped,
    meanContrast: contrast / Math.max(clearSamples, 1),
  };
}

export function assertWideSmoothContour(
  frame: PlotFrame,
  options: ContourProbeOptions = {},
) {
  const result = inspectWideSmoothContour(frame, options);
  if (options.baseline) {
    if (
      result.clearSamples < 30 ||
      result.escaped > result.clearSamples * 0.02 ||
      result.meanContrast < 20
    )
      throw new Error(
        `Glass stroke coverage or legibility changed: ${JSON.stringify(result)}`,
      );
    return result;
  }
  if (result.interiorPaleFraction > 0.04)
    throw new Error(
      `Internal contour rims cross the smooth stroke interior: ${JSON.stringify(result)}`,
    );
  if (result.innerRimPaleFraction > 0.06)
    throw new Error(
      `Pale shards extend inward from exterior joins: ${JSON.stringify(result)}`,
    );
  if (result.rimPaleFraction < 0.6)
    throw new Error(
      `Smooth stroke exterior rim was lost: ${JSON.stringify(result)}`,
    );
  return result;
}
