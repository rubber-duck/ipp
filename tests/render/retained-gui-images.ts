/** Node-side frame arithmetic and PNG artifacts for the retained GUI harness. */
import { deflateSync } from "node:zlib";

/** Completed RGBA8 frame, row zero at the top. */
export interface RgbaFrame {
  readonly width: number;
  readonly height: number;
  readonly pixels: Uint8Array;
}

const CRC_TABLE = Uint32Array.from({ length: 256 }, (_, index) => {
  let value = index;
  for (let bit = 0; bit < 8; bit++)
    value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
  return value >>> 0;
});

function crc32(bytes: Uint8Array): number {
  let crc = 0xffffffff;
  for (const byte of bytes) crc = CRC_TABLE[(crc ^ byte) & 0xff]! ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

function chunk(type: string, data: Uint8Array): Buffer {
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const header = Buffer.alloc(4);
  header.writeUInt32BE(data.length);
  const trailer = Buffer.alloc(4);
  trailer.writeUInt32BE(crc32(body));
  return Buffer.concat([header, body, trailer]);
}

/** Lossless 8-bit RGBA PNG without scanline filtering. */
export function encodePng(frame: RgbaFrame): Buffer {
  const { width, height, pixels } = frame;
  if (pixels.length !== width * height * 4)
    throw new Error(
      `RGBA frame has ${pixels.length} bytes for ${width}x${height}`,
    );
  const stride = width * 4;
  const scanlines = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y++)
    scanlines.set(
      pixels.subarray(y * stride, (y + 1) * stride),
      y * (stride + 1) + 1,
    );
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header.set([8, 6, 0, 0, 0], 8);
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", header),
    chunk("IDAT", deflateSync(scanlines)),
    chunk("IEND", new Uint8Array()),
  ]);
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
