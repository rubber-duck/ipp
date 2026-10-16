/** Stable scene detail while carousel image demand changes. */
import assert from "node:assert/strict";
import type { openApplication } from "../drivers/browser-application.js";
import type { RgbaImage } from "../../../tools/shared-host/images.js";
import type { RenderStatisticsSnapshot } from "@ipp/client/diagnostics";

type Application = Awaited<ReturnType<typeof openApplication>>;

function dashboardDifference(before: RgbaImage, after: RgbaImage) {
  assert.equal(after.width, before.width);
  assert.equal(after.height, before.height);
  let change = 0,
    detail = 0,
    samples = 0;
  // Lower console is behind the carousel and clear of the floating screens.
  for (let y = Math.floor(before.height * 0.92); y < before.height - 1; y++)
    for (let x = 1; x < before.width - 1; x++)
      for (let c = 0; c < 3; c++) {
        const i = (y * before.width + x) * 4 + c;
        change += Math.abs(before.pixels[i]! - after.pixels[i]!);
        detail += Math.abs(before.pixels[i]! - before.pixels[i - 4]!);
        samples++;
      }
  assert.ok(
    detail / samples > 0.1,
    "dashboard reference contains visible detail",
  );
  assert.ok(
    change / samples < 2,
    `scene detail changed by ${change / samples} while scrolling`,
  );
  return { difference: change / samples, detail: detail / samples };
}

export async function exerciseSurfaceQuality(
  app: Application,
  record: (name: string, data: unknown) => Promise<void>,
) {
  await app.navigate("cockpit");
  assert.equal((await app.state()).options.surfaceCaching, "off");
  await app.call("options", {
    lightsPlaying: false,
    atmospherePlaying: false,
    scanning: false,
  });
  for (const surface of ["flat", "curved"]) {
    await app.call("options", {
      surface,
      carousel: 1,
      surfaceCaching: "off",
      surfaceTextureSize: 2048,
    });
    await app.page.waitForTimeout(700);
    await app.capture(`scene-quality-${surface}-before`);
    const before = await app.call<RgbaImage>("image");
    assert.equal(before.width, 2448);
    assert.equal(before.height, 1512);
    const picker = await app.call<{ bounds: Record<string, number> }>(
      "point",
      "demo-scene-picker",
    );
    let labelPixels = 0;
    for (
      let y = Math.ceil(picker.bounds.y! * 2);
      y < (picker.bounds.y! + picker.bounds.height!) * 2;
      y++
    )
      for (
        let x = Math.ceil(picker.bounds.x! * 2);
        x < (picker.bounds.x! + picker.bounds.width!) * 2;
        x++
      ) {
        const i = (y * before.width + x) * 4;
        if (
          before.pixels[i]! > 170 &&
          before.pixels[i + 1]! > 170 &&
          before.pixels[i + 2]! > 170
        )
          labelPixels++;
      }
    assert.ok(
      labelPixels > 100,
      `root GUI label remains visible above scene depth: ${labelPixels}`,
    );
    for (const [mode, cap, delta] of [
      ["auto", 2048, 100],
      ["force", 256, -200],
      ["off", 2048, 100],
    ] as const) {
      await app.call("options", {
        surfaceCaching: mode,
        surfaceTextureSize: cap,
        surfaceRefreshHz: 0.5,
      });
      const point = await app.cockpitCoordinates(1, "cockpit-header-1");
      await app.page.mouse.move(point.x, point.y);
      const previous = Number((await app.state()).options.carousel);
      await app.page.mouse.wheel(0, delta);
      await app.page.waitForTimeout(800);
      const next = Number((await app.state()).options.carousel);
      assert.ok(Math.abs(next - previous) > 0.05, "wheel moves the carousel");
      await app.capture(`scene-quality-${surface}-${mode}-${cap}`);
      const after = await app.call<RgbaImage>("image");
      const pixels = dashboardDifference(before, after);
      const statistics =
        await app.call<RenderStatisticsSnapshot>("renderStatistics");
      await record("scene-quality", {
        surface,
        mode,
        cap,
        previous,
        next,
        pixels,
        surfaces: statistics.surfaces,
      });
    }
  }
  assert.deepEqual(app.errors, []);
}
