/** Shared observations of the real producer, Data Service and presented plot. */
import assert from "node:assert/strict";
import type { DatasetPage } from "@ipp/client";
import type { RgbaImage } from "../../../tools/shared-host/images.js";

export function assertFrameRateData(page: DatasetPage | undefined) {
  assert.ok(page, "enabled monitor owns a live dataset");
  assert.equal(page.kind, "streaming");
  assert.deepEqual(
    page.schema.map(({ name }) => name),
    ["seconds", "fps", "frame_ms"],
  );
  assert.ok(page.rows.length >= 3 && page.rows.length <= 600);
  let previous = -1;
  for (const row of page.rows) {
    const values = row.values.map((value) => Number(value.value));
    assert.ok(values[0]! > previous, "sample times increase");
    assert.ok(values[1]! > 0 && Number.isFinite(values[1]!));
    assert.ok(
      Math.abs(values[1]! * values[2]! - 1000) < 0.1,
      "FPS and mean frame interval describe the same actual completions",
    );
    previous = values[0]!;
  }
}

export function assertFrameRatePixels(
  frame: RgbaImage,
  bounds: readonly number[],
  scale = 1,
) {
  let line = 0;
  const [x, y, width, height] = bounds as [number, number, number, number];
  for (
    let py = Math.ceil((y + 24) * scale);
    py < (y + height - 30) * scale;
    py++
  )
    for (
      let px = Math.ceil((x + 44) * scale);
      px < (x + width - 10) * scale;
      px++
    ) {
      const offset = (py * frame.width + px) * 4;
      const [r, g, b] = frame.pixels.subarray(offset, offset + 3);
      if (b! > 130 && g! > 120 && b! > r! * 1.15 && g! > r! * 1.1) line++;
    }
  assert.ok(line > 15, `streamed FPS paints a visible line (${line} pixels)`);
}
