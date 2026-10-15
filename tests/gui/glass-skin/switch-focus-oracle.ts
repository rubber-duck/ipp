import type { RgbaImage } from "../../../tools/shared-host/images.js";
import { check } from "../../harness/page/checks.js";

/** Focus follows the same round sheet; its polished edge grows with the control. */
export function checkSwitchFocusScale(image: RgbaImage) {
  const density = image.width / 224;
  const evidence: Record<string, number> = {};
  const light = (x: number, y: number) => {
    const i =
      (Math.floor(y * density) * image.width + Math.floor(x * density)) * 4;
    return Math.max(...image.pixels.subarray(i, i + 3));
  };
  for (const [scale, top] of [
    [1, 12],
    [2, 64],
    [3, 144],
  ] as const) {
    const x = 12 + 62 * scale * 0.4;
    const centre = light(x, top + 10 * scale);
    let bright = 0,
      peak = 0;
    for (
      let py = Math.ceil(top * density);
      py < Math.floor((top + 6 * scale) * density);
      py++
    ) {
      const value = light(x, py / density);
      peak = Math.max(peak, value);
      if (value > centre + 15) bright++;
    }
    check(
      peak > centre + 25,
      `Switch ${scale}x lost its polished focus edge: ${peak}/${centre}`,
    );
    check(
      bright >= 0.5 * scale * density && bright <= 3.5 * scale * density,
      `Switch ${scale}x edge width did not scale: ${bright}`,
    );
    check(
      light(x, top - 3 * scale) < 35,
      `Switch ${scale}x focus detached outside the sheet`,
    );
    // The capsule's bounding-box corner is outside its curved silhouette.
    check(
      light(12 + scale, top + scale) < 35,
      `Switch ${scale}x acquired a square focus corner`,
    );
    evidence[`${scale}x-edge-pixels`] = bright;
  }
  return evidence;
}
