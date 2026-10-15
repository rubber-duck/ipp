import type { RgbaImage } from "../../../../tools/shared-host/images.js";
import { check } from "../../../harness/page/checks.js";

type Region = readonly [number, number, number, number];
const PANEL_BODY: Region = [60, 225, 280, 28];
const PANEL: Region = [24, 24, 380, 264];
const FOCUSED_CELL: Region = [40, 378, 180, 24];

function pixels(
  frame: RgbaImage,
  region: Region,
  visit: (offset: number) => void,
) {
  const [x, y, width, height] = region.map((value) =>
    Math.round(value * 2),
  ) as [number, number, number, number];
  for (let row = y; row < y + height; row++)
    for (let column = x; column < x + width; column++)
      visit((row * frame.width + column) * 4);
}

/** Independent image regions avoid counting page background and unrelated controls. */
export function containerChangedPixels(
  a: RgbaImage,
  b: RgbaImage,
  region: Region = PANEL,
): number {
  check(
    a.width === b.width && a.height === b.height,
    "Container image extents changed",
  );
  let changed = 0;
  pixels(a, region, (offset) => {
    if (
      [0, 1, 2].some(
        (channel) =>
          Math.abs(a.pixels[offset + channel]! - b.pixels[offset + channel]!) >
          2,
      )
    )
      changed++;
  });
  return changed;
}

function meanBlue(frame: RgbaImage, region: Region) {
  let sum = 0;
  let count = 0;
  pixels(frame, region, (offset) => {
    sum += frame.pixels[offset + 2]!;
    count++;
  });
  return sum / count;
}

export function checkContainerGlass(frame: RgbaImage) {
  let dark = 0;
  let total = 0;
  let brightText = 0;
  pixels(frame, PANEL_BODY, (offset) => {
    total++;
    if (Math.max(...frame.pixels.subarray(offset, offset + 3)) < 96) dark++;
    check(
      frame.pixels[offset + 3] === 255,
      "Completed frame has unexpected alpha",
    );
  });
  pixels(frame, FOCUSED_CELL, (offset) => {
    if (Math.min(...frame.pixels.subarray(offset, offset + 3)) > 150)
      brightText++;
  });
  check(dark / total > 0.9, "Panel glass body is excessively bright");
  check(brightText > 30, "Focused cell glass concealed its text");
  const selectedBlue = meanBlue(frame, [240, 384, 20, 10]);
  const ordinaryBlue = meanBlue(frame, [240, 412, 20, 10]);
  check(
    selectedBlue > ordinaryBlue + 15,
    "Selected grid row lost its distinct blue glass face",
  );
  return {
    darkFraction: dark / total,
    focusedCellTextPixels: brightText,
    selectedBlue,
    ordinaryBlue,
  };
}

export function checkContainerTransmission(a: RgbaImage, backdrop: RgbaImage) {
  const changed = containerChangedPixels(a, backdrop, PANEL_BODY);
  check(changed > 1000, `Glass panel lost transmission: ${changed} pixels`);
  return { changed };
}
