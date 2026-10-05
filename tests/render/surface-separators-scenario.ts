import assert from "node:assert/strict";
import { SEPARATOR_CASES } from "./surface-separators-fixture.js";
import { compareFrames, type RgbaFrame } from "./retained-gui-images.js";
import {
  TERMINAL_POLICY,
  type SurfaceCacheDriver,
  type CacheRecord,
} from "./surface-cache-scenario.js";

/** Invert the presentation transfer: white over black measures linear pixel area. */
function linear(value: number) {
  const encoded = value / 255;
  return encoded <= 0.04045
    ? encoded / 12.92
    : ((encoded + 0.055) / 1.055) ** 2.4;
}

function area(
  frame: RgbaFrame,
  rectangle: readonly [number, number, number, number],
) {
  const [x0, y0, x1, y1] = rectangle;
  let total = 0;
  for (let y = y0; y < y1; y++)
    for (let x = x0; x < x1; x++) {
      const offset = (y * frame.width + x) * 4;
      total +=
        (linear(frame.pixels[offset]!) +
          linear(frame.pixels[offset + 1]!) +
          linear(frame.pixels[offset + 2]!)) /
        3;
    }
  return total;
}

/** Same assertions run through worker/WebGL and native WebSocket/GLES drivers. */
export async function measureSeparators(
  driver: Pick<SurfaceCacheDriver, "call" | "capture" | "pixels">,
) {
  await driver.call("cacheSeparators");
  await driver.call("setSurfaceCache", ["surface-terminal", null]);
  const results = [];
  for (const [mode, distance] of [
    ["direct", 2],
    ["cached", 6],
    ["reduced", 12],
  ] as const) {
    if (mode === "cached")
      await driver.call("setSurfaceCache", [
        "surface-terminal",
        TERMINAL_POLICY,
      ]);
    await driver.call("cameraDistance", [distance]);
    const label = `separators-${mode}`;
    let previous: RgbaFrame | undefined;
    for (let attempt = 0; attempt < 120; attempt++) {
      const frame = await driver.capture(label, true);
      assert.equal(frame.failedDrawCalls, 0, label);
      assert.notEqual(frame.invalidCamera, true, label);
      const records = frame.statistics!.surfaces!
        .surfaceCaches as unknown as CacheRecord[];
      const ready =
        mode === "direct" ||
        records.some(
          ({ mode: state, band }) =>
            (state === "repainted" || state === "reused") &&
            band === (mode === "cached" ? 1 : 2),
        );
      const pixels = driver.pixels(label);
      if (
        ready &&
        previous &&
        compareFrames(previous, pixels, 0).changedPixels === 0
      )
        break;
      if (attempt === 119) throw new Error(`${label}: did not settle`);
      previous = pixels;
    }
    // A broad sharp rectangle has exact overlap area in its corner pixels.
    // Band 2 intentionally resamples it, so compare its total area there instead.
    const rectangle = [128.25, 196.25, 140.75, 204.75] as const;
    let cornerError = 0;
    if (mode !== "reduced")
      for (let y = 195; y < 206; y++)
        for (let x = 127; x < 142; x++) {
          const expected =
            Math.max(
              0,
              Math.min(x + 1, rectangle[2]) - Math.max(x, rectangle[0]),
            ) *
            Math.max(
              0,
              Math.min(y + 1, rectangle[3]) - Math.max(y, rectangle[1]),
            );
          cornerError = Math.max(
            cornerError,
            Math.abs(
              area(driver.pixels(label), [x, y, x + 1, y + 1]) - expected,
            ),
          );
        }
    const rectangleArea = area(driver.pixels(label), [124, 192, 145, 210]);
    results.push({
      mode,
      cornerError,
      rectangleArea,
      cases: SEPARATOR_CASES.map(
        ({ id, axis, x, y, width, height, thickness }) => {
          // Only the middle 40 px of each long line; four pixels either side include all AA and cache filtering.
          const horizontal = axis === "horizontal";
          const x0 = horizontal ? 40 : Math.floor(8 + x + width / 2) - 4;
          const y0 = horizontal ? Math.floor(24 + y + height / 2) - 4 : 84;
          const rectangle = [
            x0,
            y0,
            x0 + (horizontal ? 40 : 8),
            y0 + (horizontal ? 8 : 40),
          ] as const;
          const expected = thickness * 40;
          const actual = area(driver.pixels(label), rectangle);
          return { id, expected, actual, ratio: actual / expected, rectangle };
        },
      ),
    });
  }
  await driver.call("setSurfaceCache", ["surface-terminal", null]);
  return results;
}

export function assertSeparators(
  report: Awaited<ReturnType<typeof measureSeparators>>,
) {
  for (const { mode, cases, cornerError, rectangleArea } of report) {
    assert.ok(
      cornerError <= 0.012,
      `${mode}: sharp rectangle corner error ${cornerError}`,
    );
    assert.ok(
      Math.abs(rectangleArea / (12.5 * 8.5) - 1) <= 0.02,
      `${mode}: sharp rectangle area ${rectangleArea}`,
    );
    for (const { id, ratio } of cases)
      assert.ok(
        Math.abs(ratio - 1) <= 0.06,
        `${mode}: ${id} retained ${ratio.toFixed(4)} of authored pixel area (expected 1 ± 0.06)`,
      );
  }
}
