import type { RgbaImage } from "../../../tools/shared-host/images.js";
import { EDGE_SAMPLES } from "./specimens/edge-quality.js";

function linear(value: number) {
  const s = value / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

/** Compare the complete optical edge to a denser render resolved in linear light.
 * Alpha-only checks cannot detect a specular rim sampled at the wrong part of a pixel.
 */
export function measureGlassRadiance(
  actual: RgbaImage,
  reference: RgbaImage,
  scale: number,
) {
  const factor = reference.width / actual.width;
  if (
    !Number.isInteger(factor) ||
    factor < 2 ||
    reference.height !== actual.height * factor
  )
    throw new Error(
      "The reference must be an integer multiple of the frame density",
    );
  return EDGE_SAMPLES.map((sample) => {
    const cx = (sample.x + sample.width / 2) * scale;
    const cy = (sample.y + sample.height / 2) * scale;
    const radius = (sample.height * scale) / 2;
    const straight = ((sample.width - sample.height) * scale) / 2;
    let total = 0,
      squared = 0,
      maximum = 0,
      count = 0;
    for (
      let y = Math.floor(sample.y * scale) - 1;
      y < Math.ceil((sample.y + sample.height) * scale) + 1;
      y++
    )
      for (
        let x = Math.floor(sample.x * scale) - 1;
        x < Math.ceil((sample.x + sample.width) * scale) + 1;
        x++
      ) {
        const distance =
          Math.hypot(
            Math.max(0, Math.abs(x + 0.5 - cx) - straight),
            y + 0.5 - cy,
          ) - radius;
        if (distance < -3 * scale || distance > 1.5) continue;
        for (let c = 0; c < 3; c++) {
          let expected = 0;
          for (let dy = 0; dy < factor; dy++)
            for (let dx = 0; dx < factor; dx++)
              expected +=
                linear(
                  reference.pixels[
                    ((y * factor + dy) * reference.width + x * factor + dx) *
                      4 +
                      c
                  ]!,
                ) /
                (factor * factor);
          const delta = Math.abs(
            linear(actual.pixels[(y * actual.width + x) * 4 + c]!) - expected,
          );
          total += delta;
          squared += delta * delta;
          maximum = Math.max(maximum, delta);
          count++;
        }
      }
    return {
      sample: sample.id,
      scale,
      meanError: total / count,
      rmsError: Math.sqrt(squared / count),
      maxError: maximum,
    };
  });
}

/** Black/white transmission cancels the optical colour, exposing geometric coverage.
 * The expected curve uses independently integrated capsule area, not shader equations.
 */
export function measureGlassEdges(
  black: RgbaImage,
  white: RgbaImage,
  scale: number,
) {
  const alpha = (x: number, y: number) => {
    const at = (y * black.width + x) * 4;
    return (
      1 -
      [0, 1, 2].reduce(
        (sum, c) =>
          sum + linear(white.pixels[at + c]!) - linear(black.pixels[at + c]!),
        0,
      ) /
        3
    );
  };
  return EDGE_SAMPLES.map((sample) => {
    const cx = (sample.x + sample.width / 2) * scale,
      cy = (sample.y + sample.height / 2) * scale;
    const radius = (sample.height * scale) / 2,
      straight = ((sample.width - sample.height) * scale) / 2;
    const inside = (x: number, y: number) =>
      Math.hypot(Math.max(0, Math.abs(x - cx) - straight), y - cy) - radius;
    let opacity = 0;
    for (let y = -1; y <= 1; y++)
      for (let x = -1; x <= 1; x++)
        opacity += alpha(Math.floor(cx) + x, Math.floor(cy) + y) / 9;
    let error = 0,
      maxError = 0,
      count = 0;
    for (
      let y = Math.floor(sample.y * scale) - 2;
      y < Math.ceil((sample.y + sample.height) * scale) + 2;
      y++
    )
      for (
        let x = Math.floor(sample.x * scale) - 2;
        x < Math.ceil((sample.x + sample.width) * scale) + 2;
        x++
      ) {
        if (Math.abs(inside(x + 0.5, y + 0.5)) > 1.5) continue;
        let expected = 0;
        const samples = 32;
        for (let sy = 0; sy < samples; sy++)
          for (let sx = 0; sx < samples; sx++)
            if (inside(x + (sx + 0.5) / samples, y + (sy + 0.5) / samples) <= 0)
              expected += 1 / (samples * samples);
        const delta = Math.abs(alpha(x, y) / opacity - expected);
        error += delta;
        maxError = Math.max(maxError, delta);
        count++;
      }
    return {
      sample: sample.id,
      scale,
      opacity,
      meanError: error / count,
      maxError,
    };
  });
}
