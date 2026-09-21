import type { FrameCapture } from "@ipp/client";

const EM_PIXELS = 1024;
const ORIGIN = [512, 1280] as const;
const glyphMasks = new Map<string, ImageData>();

/** Original TTF rasterization, independent of converted curves and GPU coverage. */
function glyphMask(glyph: string): ImageData {
  let mask = glyphMasks.get(glyph);
  if (!mask) {
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 2048;
    const context = canvas.getContext("2d")!;
    context.font = `${EM_PIXELS}px SurfaceOracle`;
    context.textBaseline = "alphabetic";
    context.fillStyle = "white";
    context.fillText(glyph, ...ORIGIN);
    mask = context.getImageData(0, 0, canvas.width, canvas.height);
    glyphMasks.set(glyph, mask);
  }
  return mask;
}

function linear(encoded: number): number {
  const value = encoded / 255;
  return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
}

/** Sample the original font through an independently inverted plane projection. */
export function compareGlyph(
  frame: FrameCapture,
  glyph: string,
  fontSize: number,
  angle: number,
  perspective: boolean,
  ascender: number,
) {
  const mask = glyphMask(glyph);
  const actual = new Uint8Array(frame.pixels);
  const reference = new ImageData(frame.width, frame.height);
  const cosine = Math.cos(angle),
    sine = Math.sin(angle);
  const focal = 1 / Math.tan(0.49 / 2);
  const samples = 8;
  let interior = 0,
    missingInterior = 0,
    expectedMass = 0,
    error = 0,
    expectedTop = frame.height,
    expectedBottom = -1,
    actualTop = frame.height,
    actualBottom = -1;
  for (let y = 0; y < frame.height; y++) {
    for (let x = 0; x < frame.width; x++) {
      let coverage = 0;
      for (let sy = 0; sy < samples; sy++) {
        for (let sx = 0; sx < samples; sx++) {
          const px = x + (sx + 0.5) / samples;
          const py = y + (sy + 0.5) / samples;
          const ndcX = (2 * px) / frame.width - 1;
          const ndcY = 1 - (2 * py) / frame.height;
          const localX = perspective
            ? (6 * ndcX) /
              ((focal * cosine * frame.height) / frame.width - ndcX * sine)
            : ((px - frame.width / 2) * 3) / frame.height / cosine;
          const localY = perspective
            ? (ndcY * (6 + localX * sine)) / focal
            : ((frame.height / 2 - py) * 3) / frame.height;
          const mx = Math.floor(
            ORIGIN[0] + (localX / fontSize + 0.3) * EM_PIXELS,
          );
          const my = Math.floor(
            ORIGIN[1] - (localY / fontSize + 0.25 + ascender) * EM_PIXELS,
          );
          if (mx >= 0 && mx < mask.width && my >= 0 && my < mask.height)
            coverage += mask.data[(my * mask.width + mx) * 4 + 3]! / 255;
        }
      }
      coverage /= samples ** 2;
      const offset = (y * frame.width + x) * 4;
      const rendered = linear(actual[offset]!);
      if (coverage > 64 / 255) {
        expectedTop = Math.min(expectedTop, y);
        expectedBottom = Math.max(expectedBottom, y);
      }
      if (rendered > 64 / 255) {
        actualTop = Math.min(actualTop, y);
        actualBottom = Math.max(actualBottom, y);
      }
      if (coverage > 0.95) {
        interior++;
        if (rendered < 0.5) missingInterior++;
      }
      // Restrict error to the glyph and its neighbouring pixels, excluding the plane edge.
      if (
        Math.abs(x - frame.width / 2) < frame.width / 4 &&
        Math.abs(y - frame.height / 2) < frame.height / 3
      ) {
        expectedMass += coverage;
        error += Math.abs(coverage - rendered);
      }
      const encoded =
        coverage <= 0.0031308
          ? coverage * 12.92
          : 1.055 * coverage ** (1 / 2.4) - 0.055;
      reference.data.set(
        [encoded * 255, encoded * 255, encoded * 255, 255],
        offset,
      );
    }
  }
  const canvas = document.createElement("canvas");
  canvas.width = frame.width;
  canvas.height = frame.height;
  canvas.getContext("2d")!.putImageData(reference, 0, 0);
  return {
    canvas,
    result: {
      interior,
      missingInterior,
      expectedMass,
      relativeError: error / expectedMass,
      expectedVerticalBounds:
        expectedBottom < 0 ? null : [expectedTop, expectedBottom],
      actualVerticalBounds: actualBottom < 0 ? null : [actualTop, actualBottom],
    },
  };
}
