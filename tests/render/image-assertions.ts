export const VIEWPORT = Object.freeze({ width: 320, height: 240 });
export const BACKGROUND_RGB = Object.freeze([10, 14, 20] as const);

export interface FramePixels {
  readonly width: number;
  readonly height: number;
  readonly pixels: ArrayBuffer;
}

export interface ImageSummary {
  readonly width: number;
  readonly height: number;
  readonly foregroundPixels: number;
  readonly coverage: number;
  readonly centroidX: number | null;
  readonly centroidY: number | null;
  readonly bounds: {
    readonly left: number;
    readonly top: number;
    readonly right: number;
    readonly bottom: number;
  } | null;
  readonly meanRgb: readonly [number, number, number];
}

export interface ImageDifference {
  readonly changedPixels: number;
  readonly changedFraction: number;
  readonly meanAbsoluteChannelDifference: number;
}

export function summarizeImage(frame: FramePixels): ImageSummary {
  const pixels = new Uint8Array(frame.pixels);
  const expectedLength = frame.width * frame.height * 4;
  if (pixels.byteLength !== expectedLength) {
    throw new Error(
      `RGBA frame has ${pixels.byteLength} bytes; expected ${expectedLength}`,
    );
  }

  let foregroundPixels = 0;
  let sumX = 0;
  let sumY = 0;
  let sumR = 0;
  let sumG = 0;
  let sumB = 0;
  let left = frame.width;
  let top = frame.height;
  let right = -1;
  let bottom = -1;

  for (let y = 0; y < frame.height; y += 1) {
    for (let x = 0; x < frame.width; x += 1) {
      const offset = (y * frame.width + x) * 4;
      const r = pixels[offset] ?? 0;
      const g = pixels[offset + 1] ?? 0;
      const b = pixels[offset + 2] ?? 0;
      const foreground =
        Math.max(
          Math.abs(r - BACKGROUND_RGB[0]),
          Math.abs(g - BACKGROUND_RGB[1]),
          Math.abs(b - BACKGROUND_RGB[2]),
        ) > 8;
      if (!foreground) continue;

      foregroundPixels += 1;
      sumX += x;
      sumY += y;
      sumR += r;
      sumG += g;
      sumB += b;
      left = Math.min(left, x);
      top = Math.min(top, y);
      right = Math.max(right, x);
      bottom = Math.max(bottom, y);
    }
  }

  return {
    width: frame.width,
    height: frame.height,
    foregroundPixels,
    coverage: foregroundPixels / (frame.width * frame.height),
    centroidX: foregroundPixels === 0 ? null : sumX / foregroundPixels,
    centroidY: foregroundPixels === 0 ? null : sumY / foregroundPixels,
    bounds: foregroundPixels === 0 ? null : { left, top, right, bottom },
    meanRgb:
      foregroundPixels === 0
        ? BACKGROUND_RGB
        : [
            sumR / foregroundPixels,
            sumG / foregroundPixels,
            sumB / foregroundPixels,
          ],
  };
}

export function compareImages(
  first: FramePixels,
  second: FramePixels,
): ImageDifference {
  if (first.width !== second.width || first.height !== second.height) {
    throw new Error("image dimensions differ");
  }
  const a = new Uint8Array(first.pixels);
  const b = new Uint8Array(second.pixels);
  if (a.byteLength !== b.byteLength) throw new Error("image lengths differ");

  let changedPixels = 0;
  let absoluteDifference = 0;
  for (let offset = 0; offset < a.byteLength; offset += 4) {
    let changed = false;
    for (let channel = 0; channel < 3; channel += 1) {
      const difference = Math.abs(
        (a[offset + channel] ?? 0) - (b[offset + channel] ?? 0),
      );
      absoluteDifference += difference;
      changed ||= difference > 6;
    }
    if (changed) changedPixels += 1;
  }

  const totalPixels = first.width * first.height;
  return {
    changedPixels,
    changedFraction: changedPixels / totalPixels,
    meanAbsoluteChannelDifference: absoluteDifference / (totalPixels * 3),
  };
}

export function requireVisible(summary: ImageSummary, label: string): void {
  if (summary.coverage < 0.015 || summary.coverage > 0.7) {
    throw new Error(
      `${label} foreground coverage ${summary.coverage.toFixed(4)} is not a visible bounded object`,
    );
  }
  if (summary.bounds === null || summary.centroidX === null) {
    throw new Error(`${label} has no measurable foreground`);
  }
}

export function requireBlank(summary: ImageSummary, label: string): void {
  if (summary.coverage > 0.002) {
    throw new Error(
      `${label} foreground coverage ${summary.coverage.toFixed(4)} is not blank`,
    );
  }
}

export function requireShiftedAndScaled(
  base: ImageSummary,
  moved: ImageSummary,
): void {
  requireVisible(base, "base frame");
  requireVisible(moved, "moved frame");
  if (base.centroidX === null || moved.centroidX === null) {
    throw new Error("transform comparison has no centroid");
  }
  if (Math.abs(moved.centroidX - base.centroidX) < 8) {
    throw new Error("Transform.x did not move the rendered image horizontally");
  }
  if (moved.coverage >= base.coverage * 0.75) {
    throw new Error("Transform scale did not reduce rendered coverage");
  }
}
