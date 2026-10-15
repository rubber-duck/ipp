import type { RgbaImage } from "../../../tools/shared-host/images.js";
import { check } from "../../harness/page/checks.js";

/** Moving a point source changes the round dielectric reflection, never its bounds. */
export function checkSwitchRimLight(image: RgbaImage) {
  const density = image.width / 320;
  const differences: number[] = [];
  for (let source = 1; source <= 3; source++) {
    let total = 0,
      count = 0,
      changed = 0;
    for (let y = 14 * density; y < 68 * density; y++)
      for (let x = 18 * density; x < 142 * density; x++) {
        const a = (y * image.width + x) * 4;
        const b = ((y + source * 88 * density) * image.width + x) * 4;
        const delta = Math.max(
          ...[0, 1, 2].map((c) =>
            Math.abs(image.pixels[a + c]! - image.pixels[b + c]!),
          ),
        );
        total += delta;
        count++;
        if (delta > 3) changed++;
      }
    check(
      changed > 100,
      `Source ${source} did not move the glass highlight: ${changed}`,
    );
    differences.push(total / count);
    const outside =
      ((source * 88 + 10) * density * image.width + 80 * density) * 4;
    check(
      Math.max(...image.pixels.subarray(outside, outside + 3)) < 35,
      `Source ${source} painted outside the sheet`,
    );
  }
  check(
    Math.max(...differences) - Math.min(...differences) > 0.1,
    "Different light directions produced the same reflection",
  );
  return { differences };
}
