/** Node-side frame arithmetic and PNG artifacts for completed captures. */
import {
  decodePng,
  encodePng as encodeRgbaPng,
} from "../../tools/shared-host/png.js";

export { decodePng };

/** Completed RGBA8 frame, row zero at the top. */
export interface RgbaFrame {
  readonly width: number;
  readonly height: number;
  readonly pixels: Uint8Array;
}

/**
 * Lossless 8-bit RGBA PNG of a completed frame, through the shared encoder.
 * Artifacts are compared by decoded pixels, never by file bytes.
 */
export function encodePng(frame: RgbaFrame): Buffer {
  return encodeRgbaPng({
    width: frame.width,
    height: frame.height,
    pixels: new Uint8Array(frame.pixels),
  });
}

function sameSize(a: RgbaFrame, b: RgbaFrame): void {
  if (a.width !== b.width || a.height !== b.height)
    throw new Error(
      `Frames differ in size: ${a.width}x${a.height} and ${b.width}x${b.height}`,
    );
}

/** Largest RGB channel difference of one pixel. */
export function pixelDifference(
  a: RgbaFrame,
  b: RgbaFrame,
  index: number,
): number {
  const offset = index * 4;
  return Math.max(
    Math.abs(a.pixels[offset]! - b.pixels[offset]!),
    Math.abs(a.pixels[offset + 1]! - b.pixels[offset + 1]!),
    Math.abs(a.pixels[offset + 2]! - b.pixels[offset + 2]!),
  );
}

/**
 * Amplified grey differences, with pixels above `threshold` marked red. The
 * image is an artifact for review; assertions use explicit statistics.
 */
export function differenceImage(
  expected: RgbaFrame,
  actual: RgbaFrame,
  threshold: number,
): RgbaFrame {
  sameSize(expected, actual);
  const pixels = new Uint8Array(expected.pixels.length);
  for (let index = 0; index < expected.width * expected.height; index++) {
    const difference = pixelDifference(expected, actual, index);
    const grey = Math.min(255, difference * 4);
    pixels.set(
      difference > threshold
        ? [255, grey >> 2, grey >> 2, 255]
        : [grey, grey, grey, 255],
      index * 4,
    );
  }
  return { width: expected.width, height: expected.height, pixels };
}

/** Pixels whose colour classifies as text or other selected content. */
export function mask(
  frame: RgbaFrame,
  select: (r: number, g: number, b: number) => boolean,
): Uint8Array {
  const result = new Uint8Array(frame.width * frame.height);
  for (let index = 0; index < result.length; index++) {
    const offset = index * 4;
    result[index] = select(
      frame.pixels[offset]!,
      frame.pixels[offset + 1]!,
      frame.pixels[offset + 2]!,
    )
      ? 1
      : 0;
  }
  return result;
}

export function count(values: Uint8Array): number {
  let total = 0;
  for (const value of values) total += value;
  return total;
}

/** Inclusive pixel bounds `[left, top, right, bottom]` of a mask, or null when empty. */
export function maskBounds(
  values: Uint8Array,
  width: number,
): [number, number, number, number] | null {
  let bounds: [number, number, number, number] | null = null;
  for (let index = 0; index < values.length; index++) {
    if (!values[index]) continue;
    const [x, y] = [index % width, Math.floor(index / width)];
    bounds = bounds
      ? [
          Math.min(bounds[0], x),
          Math.min(bounds[1], y),
          Math.max(bounds[2], x),
          Math.max(bounds[3], y),
        ]
      : [x, y, x, y];
  }
  return bounds;
}

/** Intersection over union of two masks; empty masks agree completely. */
export function intersectionOverUnion(a: Uint8Array, b: Uint8Array): number {
  let intersection = 0;
  let union = 0;
  for (let index = 0; index < a.length; index++) {
    if (a[index] || b[index]) union++;
    if (a[index] && b[index]) intersection++;
  }
  return union === 0 ? 1 : intersection / union;
}

export interface FrameDifference {
  readonly changedPixels: number;
  readonly changedFraction: number;
  readonly meanChannelDifference: number;
  readonly maxChannelDifference: number;
}

export function compareFrames(
  expected: RgbaFrame,
  actual: RgbaFrame,
  threshold: number,
): FrameDifference {
  sameSize(expected, actual);
  const total = expected.width * expected.height;
  let changedPixels = 0;
  let sum = 0;
  let max = 0;
  for (let index = 0; index < total; index++) {
    const difference = pixelDifference(expected, actual, index);
    if (difference > threshold) changedPixels++;
    sum += difference;
    max = Math.max(max, difference);
  }
  return {
    changedPixels,
    changedFraction: changedPixels / total,
    meanChannelDifference: sum / total,
    maxChannelDifference: max,
  };
}
