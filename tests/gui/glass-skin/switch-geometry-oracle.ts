import type { RgbaImage } from "../../../tools/shared-host/images.js";
import { check } from "../../harness/page/checks.js";

/** Find the solid thumb, ignoring short bright runs from the track's rim. */
function widestRun(length: number, inside: (offset: number) => boolean) {
  let start = 0;
  let best = [0, 0];
  for (let end = 0; end <= length; end++) {
    if (end < length && inside(end)) continue;
    if (end - start > best[1]! - best[0]!) best = [start, end];
    start = end + 1;
  }
  return best.map((value) => value / 2);
}

/** Independent image measurements: two-unit end/side clearance at both scales. */
export function checkSwitchGeometry(frame: RgbaImage) {
  check(
    frame.width === 1280 && frame.height === 544,
    "Switch reference DPR2 extent",
  );
  for (const column of [0, 1])
    for (const scale of [1, 2])
      for (const state of [0, 1]) {
        const x = 32 + column * 320 + state * 144;
        const y = 80 + (scale - 1) * 96;
        const width = 40 * scale;
        const height = 22 * scale;
        const thumb = (dx: number, dy: number) => {
          const at =
            (Math.floor((y + dy) * 2) * frame.width +
              Math.floor((x + dx) * 2)) *
            4;
          return column === 0
            ? frame.pixels[at + 1]! > (state ? 200 : 110)
            : frame.pixels[at]! > 100;
        };
        const horizontal = widestRun(width * 2, (pixel) =>
          thumb((pixel + 0.5) / 2, height / 2),
        );
        const vertical = widestRun(height * 2, (pixel) =>
          thumb((state ? 29 : 11) * scale, (pixel + 0.5) / 2),
        );
        const expectedX = (state ? 20 : 2) * scale;
        const actual = [...horizontal, ...vertical];
        const expected = [
          expectedX,
          expectedX + 18 * scale,
          2 * scale,
          20 * scale,
        ];
        check(
          actual.every(
            (value, index) =>
              Math.abs(value - expected[index]!) <= 0.75 * scale,
          ),
          `${column ? "Glass" : "Cybertron"} ${scale}x ${state ? "on" : "off"} thumb bounds ${actual} vs ${expected}`,
        );
      }
}
