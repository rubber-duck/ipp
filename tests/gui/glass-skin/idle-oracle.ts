/** Image checks for restrained idle reflections, independent of shader parameters. */
import type { RgbaImage } from "../../../tools/shared-host/images.js";
import { check } from "../../harness/page/checks.js";

export type ControlRect = readonly [number, number, number, number];

function brightness(image: RgbaImage, [x, y, width, height]: ControlRect) {
  let total = 0,
    peak = 0,
    count = 0;
  for (let row = Math.floor(y); row < Math.ceil(y + height); row++)
    for (let column = Math.floor(x); column < Math.ceil(x + width); column++) {
      const at = (row * image.width + column) * 4;
      const level =
        image.pixels[at]! * 0.2126 +
        image.pixels[at + 1]! * 0.7152 +
        image.pixels[at + 2]! * 0.0722;
      total += level;
      peak = Math.max(peak, level);
      count++;
    }
  check(count > 0, "Idle material probe has no pixels");
  return { mean: total / count, peak };
}

/** The upper middle edge excludes the label, check mark and rounded corners. */
export function glassRimBrightness(
  image: RgbaImage,
  [x, y, width]: ControlRect,
) {
  return brightness(image, [x + width * 0.2, y, width * 0.6, 3]);
}

/** The dial's value annulus surrounds its central readout inside the native field. */
function dialRimBrightness(
  image: RgbaImage,
  [x, y, width, height]: ControlRect,
) {
  const radius = Math.min(width, height) / 2;
  const cx = x + width / 2,
    cy = y + height / 2;
  let total = 0,
    peak = 0,
    count = 0;
  for (let row = Math.floor(y); row < Math.ceil(y + height); row++)
    for (let column = Math.floor(x); column < Math.ceil(x + width); column++) {
      const distance = Math.hypot(column + 0.5 - cx, row + 0.5 - cy);
      if (distance < radius * 0.65 || distance > radius) continue;
      const level = brightness(image, [column, row, 1, 1]).mean;
      total += level;
      peak = Math.max(peak, level);
      count++;
    }
  check(count > 0, "Dial rim probe has no pixels");
  return { mean: total / count, peak };
}

/** Bright semantic backing is allowed; the optical perimeter must stay quiet. */
export function checkQuietGlassIdle(
  image: RgbaImage,
  bounds: Readonly<Record<string, ControlRect>>,
) {
  const limits = {
    // Liquid glass keeps a narrow polished tangent, with no external halo.
    button: 105,
    input: 115,
    dial: 150,
    checkbox: 165,
    switch: 165,
  };
  const report: Record<string, ReturnType<typeof glassRimBrightness>> = {};
  for (const [name, limit] of Object.entries(limits)) {
    const [bx, by, bw, bh] = bounds[name]!;
    // Compact checkboxes keep a 28-unit hit target around an 18-unit square.
    // Switch tracks use their actual native height, with no hidden paint inset.
    const ix = name === "checkbox" ? (Math.min(bw, bh) * 5) / 28 : 0;
    const iy = name === "checkbox" ? ix : 0;
    const rect: ControlRect = [bx + ix, by + iy, bw - 2 * ix, bh - 2 * iy];
    const rim =
      name === "dial"
        ? dialRimBrightness(image, rect)
        : glassRimBrightness(image, rect);
    check(
      rim.peak < limit,
      `Idle ${name} has a luminous perimeter: ${rim.peak.toFixed(1)} >= ${limit}`,
    );
    // Use the left side: a dial's authored caption can sit directly above it.
    const [x, y, , height] = rect;
    const outside = brightness(image, [
      x - 3,
      y + height * 0.2,
      1,
      height * 0.6,
    ]);
    const backdrop = brightness(image, [
      x - 6,
      y + height * 0.2,
      1,
      height * 0.6,
    ]);
    check(
      Math.abs(outside.mean - backdrop.mean) < 3,
      `Idle ${name} spills a halo outside its contour`,
    );
    report[name] = rim;
  }
  check(
    report.switch!.mean > report.button!.mean + 12,
    "Checked state lost its useful backing tint",
  );
  return report;
}
