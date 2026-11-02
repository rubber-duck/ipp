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

function headerDifference(
  before: RgbaImage,
  after: RgbaImage,
  bounds: readonly number[],
) {
  assert.equal(after.width, before.width);
  assert.equal(after.height, before.height);
  let difference = 0,
    bright = 0,
    samples = 0;
  for (
    let y = Math.max(0, Math.ceil(bounds[1]!));
    y < Math.min(before.height, bounds[3]!);
    y++
  )
    for (
      let x = Math.max(0, Math.ceil(bounds[0]!));
      x < Math.min(before.width, bounds[2]!);
      x++
    ) {
      const i = (y * before.width + x) * 4;
      if (
        before.pixels[i]! > 70 &&
        before.pixels[i + 1]! > 130 &&
        before.pixels[i + 2]! > 140
      )
        bright++;
      for (let c = 0; c < 3; c++)
        difference += Math.abs(before.pixels[i + c]! - after.pixels[i + c]!);
      samples += 3;
    }
  assert.ok(bright > 30, "panel header reference contains readable cyan text");
  return { difference: difference / samples, bright, samples };
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
  const stopped = () =>
    app.waitFor(
      (state) =>
        state.controllers?.every(
          (controller) =>
            !controller.transition &&
            (!controller.motion || controller.state !== "playing"),
        ) ?? false,
    );
  await app.call("startCockpitInputTrace");
  for (const surface of ["flat", "curved"]) {
    await app.call("options", {
      surface,
      carousel: 1,
      surfaceCaching: "off",
      surfaceTextureSize: 2048,
    });
    await stopped();
    await app.capture(`scene-quality-${surface}-before`);
    const before = await app.call<RgbaImage>("image");
    assert.equal(before.width, 2448);
    assert.equal(before.height, 1512);
    const canvas = await app.page.locator("#ipp-demo-canvas").boundingBox();
    assert.ok(canvas);
    const corners = [];
    for (const u of [0, 0.5, 1])
      for (const v of [0, 1])
        corners.push(await app.cockpitCoordinates(1, "cockpit-header-1", u, v));
    const header = [
      ((Math.min(...corners.map((p) => p.x)) - canvas.x) * before.width) /
        canvas.width,
      ((Math.min(...corners.map((p) => p.y)) - canvas.y) * before.height) /
        canvas.height,
      ((Math.max(...corners.map((p) => p.x)) - canvas.x) * before.width) /
        canvas.width,
      ((Math.max(...corners.map((p) => p.y)) - canvas.y) * before.height) /
        canvas.height,
    ];
    for (const [mode, cap] of [
      ["force", 256],
      ["off", 256],
      ["off", 2048],
    ] as const) {
      await app.call("options", {
        surfaceCaching: mode,
        surfaceTextureSize: cap,
      });
      await app.capture(`panel-detail-${surface}-${mode}-${cap}`);
      const frame = await app.call<RgbaImage>("image");
      const pixels = headerDifference(before, frame, header);
      const work = (
        await app.call<RenderStatisticsSnapshot>("renderStatistics")
      ).surfaces;
      if (mode === "off") {
        assert.ok(
          pixels.difference < 0.5,
          `Off preserves direct header detail at ${cap}px: ${JSON.stringify(pixels)}`,
        );
        assert.equal(
          work.surfaceCacheRepaints + work.surfaceCacheReuses,
          0,
          "Off presents panels without raster images",
        );
      } else {
        assert.ok(
          pixels.difference > 0.5,
          "Force applies the low-resolution image cap to the panel",
        );
        assert.ok(work.surfaceCacheRepaints + work.surfaceCacheReuses > 0);
      }
      await record("panel-detail", {
        surface,
        mode,
        cap,
        header,
        pixels,
        surfaces: work,
      });
    }
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
      ["auto", 2048, 600],
      ["force", 256, -1200],
      ["off", 2048, 600],
    ] as const) {
      await app.call("options", {
        surfaceCaching: mode,
        surfaceTextureSize: cap,
        surfaceRefreshHz: 0.5,
      });
      // Target the centered pane after each spring settles. In this DPR-2
      // fixture, six injected notches yield 120 logical units and cross the
      // half-pane selection threshold.
      const previous = Number((await app.state()).options.carousel);
      const selected = Math.round(previous);
      const point = await app.cockpitCoordinates(
        selected,
        `cockpit-header-${selected}`,
      );
      await app.page.mouse.move(point.x, point.y);
      await app.page.mouse.wheel(0, delta);
      const deadline = performance.now() + 30_000;
      let next: number;
      for (;;) {
        next = Number((await app.state()).options.carousel);
        if (Number.isInteger(next) && Math.abs(next - previous) > 0.05) break;
        if (performance.now() >= deadline) {
          await record("wheel-selection-timeout", {
            surface,
            mode,
            previous,
            next,
            point,
            diagnostics: await app.call("cockpitInputDiagnostics"),
          });
          assert.fail(
            "wheel selects a different pane before the input deadline",
          );
        }
      }
      await stopped();
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
