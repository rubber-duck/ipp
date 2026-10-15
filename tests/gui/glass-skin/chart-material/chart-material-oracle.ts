import assert from "node:assert/strict";
import type {
  PixelRect,
  RgbaImage,
} from "../../../../tools/shared-host/images.js";
import {
  bar2d,
  bar3d,
  CALLOUT2,
  corners3d,
  EXTENT,
  FRAME,
  PLOT,
  project,
} from "./mapping.js";

function difference(a: RgbaImage, b: RgbaImage, rect: PixelRect) {
  let total = 0;
  const [left, top, width, height] = rect;
  for (let y = top; y < top + height; y++)
    for (let x = left; x < left + width; x++) {
      const offset = (y * a.width + x) * 4;
      for (let c = 0; c < 3; c++)
        total += Math.abs(a.pixels[offset + c]! - b.pixels[offset + c]!);
    }
  return total / (width * height * 3);
}
function pixel(image: RgbaImage, x: number, y: number) {
  const offset = (Math.round(y) * image.width + Math.round(x)) * 4;
  return Array.from(image.pixels.subarray(offset, offset + 3));
}
function rect(x: number, y: number, width: number, height: number): PixelRect {
  return [x, y, width, height].map((v) =>
    Math.round(v * 2),
  ) as unknown as PixelRect;
}
function runs(length: number, test: (i: number) => boolean) {
  const result: { start: number; length: number }[] = [];
  let start = -1;
  for (let i = 0; i <= length; i++) {
    if (i < length && test(i)) {
      if (start < 0) start = i;
    } else if (start >= 0) {
      result.push({ start, length: i - start });
      start = -1;
    }
  }
  return result;
}
function samePixel(a: RgbaImage, b: RgbaImage, x: number, y: number) {
  const p = (y * a.width + x) * 4;
  return [0, 1, 2].every(
    (c) => Math.abs(a.pixels[p + c]! - b.pixels[p + c]!) <= 1,
  );
}
function enclosed(corners: readonly (readonly number[])[]) {
  return corners.every(
    (p) =>
      p[0]! >= 0 &&
      p[0]! <= FRAME.width &&
      p[1]! >= 0 &&
      p[1]! <= FRAME.height &&
      p[2]! >= 0 &&
      p[2]! <= FRAME.depth,
  );
}
function overlaps(
  a: { x: number; y: number; width: number; height: number },
  b: { x: number; y: number; width: number; height: number },
) {
  return (
    a.x < b.x + b.width &&
    a.x + a.width > b.x &&
    a.y < b.y + b.height &&
    a.y + a.height > b.y
  );
}
/** Completed-frame opacity masks test the rendered silhouette, not just authored heights. */
export function checkMaterialMatrix(
  images: Readonly<Record<string, RgbaImage>>,
) {
  const reports = [];
  for (const dimension of [2, 3]) {
    const get = (suffix = "") => {
      const image = images[`bars-${dimension}d${suffix}`];
      assert(image, `Missing ${dimension}D ${suffix} image`);
      assert.equal(image.width, EXTENT.width);
      assert.equal(image.height, EXTENT.height);
      for (let i = 0; i < image.pixels.length; i += 4)
        assert(
          !(
            image.pixels[i]! > 220 &&
            image.pixels[i + 1]! < 20 &&
            image.pixels[i + 2]! < 20
          ),
          `${dimension}D red shader fallback`,
        );
      return image;
    };
    const base = get(),
      right = get("-right"),
      violet = get("-violet"),
      contrast = get("-contrast");
    const front = project([1, 0.55, 5.6]),
      selectedFace = project([3, 1.2, 1.6]);
    const body =
      dimension === 2
        ? rect(220, 245, 30, 90)
        : rect(selectedFace[0] - 4, selectedFace[1] - 8, 8, 16);
    const facets =
      dimension === 2
        ? rect(205, 195, 70, 38)
        : rect(front[0] - 8, front[1] - 34, 18, 38);
    const opacity = difference(base, contrast, body),
      background = difference(base, contrast, rect(40, 380, 20, 20));
    const light = difference(base, right, facets),
      palette = difference(base, violet, body);
    assert(
      background > 35,
      `${dimension}D contrast background did not change: ${background}`,
    );
    assert(opacity < 0.1, `${dimension}D body leaks background: ${opacity}`);
    assert(
      light > 0.5,
      `${dimension}D facets do not respond to source: ${light}`,
    );
    assert(palette > 20, `${dimension}D palette did not change: ${palette}`);
    reports.push({
      dimension,
      bodyBackgroundDifference: opacity,
      backgroundDifference: background,
      lightDifference: light,
      paletteDifference: palette,
    });
  }
  const base = images["bars-2d"]!,
    contrast = images["bars-2d-contrast"]!;
  const left = PLOT.x * 2,
    top = PLOT.y * 2,
    bottom = (PLOT.y + PLOT.height) * 2;
  const widths = runs(PLOT.width * 2, (i) =>
    samePixel(base, contrast, left + i, 740),
  ).filter((run) => run.length > 20);
  assert.equal(
    widths.length,
    6,
    "Six opaque rendered bars, including both endpoints",
  );
  for (const run of widths)
    assert(
      Math.abs(run.length - 144) <= 2,
      `Clipped or unequal rendered bar width: ${run.length}`,
    );
  assert(
    widths[0]!.start >= 30 &&
      widths[5]!.start + widths[5]!.length <= PLOT.width * 2 - 30,
    "Visible end margins",
  );
  const expectedHeights = [147.84, 394.24, 258.72, 505.12, 338.8, 455.84];
  const heights = [];
  for (let i = 0; i < 6; i++) {
    const b = bar2d(i),
      x = Math.round((b.x + b.width * 0.23) * 2);
    const run = runs(bottom - top + 5, (j) =>
      samePixel(base, contrast, x, top + j),
    ).sort((a, b) => b.length - a.length)[0]!;
    assert(
      Math.abs(run.length - expectedHeights[i]!) <= 2,
      `Rendered height ${i}: ${run.length}`,
    );
    assert(
      Math.abs(top + run.start + run.length - bottom) <= 1,
      `Rendered baseline ${i}`,
    );
    assert(!overlaps(b, CALLOUT2), `Callout obscures bar ${i}`);
    heights.push(run.length);
  }
  const footprint = [];
  for (let i = 0; i < 12; i++) {
    assert(enclosed(corners3d(i)), `3D bar ${i} exceeds the grid`);
    const b = bar3d(i);
    footprint.push({
      x: [b.x - b.width / 2, b.x + b.width / 2],
      z: [b.z - b.depth / 2, b.z + b.depth / 2],
    });
  }
  // Negative controls encode the rejected boundary-centered cubes and overlapping tooltip.
  assert(
    !enclosed([
      [-0.425, 0, -0.5],
      [0.425, 0.72, 0.5],
    ]),
    "Containment must reject original geometry",
  );
  assert(
    overlaps(bar2d(3), { x: 405, y: 125, width: 130, height: 50 }),
    "Collision check rejects the original tooltip placement",
  );
  const selected = bar2d(1),
    ordinary = bar2d(3);
  const fill = pixel(
    base,
    (selected.x + selected.width * 0.5) * 2,
    (selected.y + selected.height * 0.14) * 2,
  );
  const blue = pixel(
    base,
    (ordinary.x + ordinary.width * 0.5) * 2,
    (ordinary.y + ordinary.height * 0.14) * 2,
  );
  // Broad tolerances around independent approved concept samples [0,183,230] and [21,115,170].
  assert(
    Math.abs(fill[1]! - 183) < 20 && Math.abs(fill[2]! - 230) < 22,
    `Selected face differs from concept lightness: ${fill}`,
  );
  assert(
    Math.abs(blue[1]! - 115) < 22 && Math.abs(blue[2]! - 170) < 24,
    `Ordinary face differs from concept blue: ${blue}`,
  );
  const halo = pixel(base, (selected.x - 3) * 2, 560),
    far = pixel(base, (selected.x - 8) * 2, 560);
  assert(
    halo[1]! > far[1]! + 3 && halo[1]! < 50,
    `Halo should be visible and narrow: ${halo} / ${far}`,
  );
  return {
    materials: reports,
    renderedWidths: widths.map((r) => r.length),
    renderedHeights: heights,
    footprints: footprint,
    conceptSamples: { selected: fill, ordinary: blue, halo, far },
  };
}
