import assert from "node:assert/strict";
import test from "node:test";
import { decodePng } from "../../tools/shared-host/png.js";
import { runBrowserEnvironment } from "../browser/environment.js";
import {
  exerciseGalleryCharts,
  exerciseChartFeedbackEdits,
  baselineBarPointer,
  baselineGridPointer,
  baselineSingleRowPointer,
  focusChart,
  waitForCharts,
  assertCenterCamera,
  type GalleryChartsDriver,
  type GalleryChartsState,
} from "../integration/scenarios/gallery-charts.js";
import {
  exerciseStreamingCharts,
  type StreamingChartsDriver,
  type StreamingChartsState,
} from "../integration/scenarios/gallery-chart-streaming.js";
import { openGallery, galleryEnvironment } from "./gallery-driver.js";

test("unified charts retain fixed samples with free camera angles, two second focus and source row interaction", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "unified gallery charts",
    galleryEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      await g.navigate("charts");
      assert.equal((await g.call<string[]>("galleryWorlds")).length, 11);
      const driver: GalleryChartsDriver = {
        inspect: () => g.call<GalleryChartsState>("gallerySceneState"),
        action: (name, args) => g.call("gallerySceneAction", name, args),
        capture: async (label) => {
          const captured = await g.capture(label);
          if (
            [
              "ring-center",
              "ring-whole",
              "charts-focus-straight",
              "charts-focus-grid-bars",
              "charts-focus-bars",
              "charts-focus-single-row",
              "charts-focus-height-surface",
              "charts-free-angle",
              "charts-inward-oblique",
            ].includes(label)
          )
            await g.page.screenshot({
              path: `${scenario.evidence.directory}/${label}-page.png`,
              fullPage: true,
            });
          return decodePng(
            Buffer.from(captured.dataUrl.split(",")[1]!, "base64"),
          );
        },
        record: async (label, value) => {
          await scenario.evidence.writeJson(`${label}.json`, value);
          await scenario.evidence.record(label, { artifact: `${label}.json` });
        },
      };
      await exerciseGalleryCharts(driver);
      const canvasBounds = await g.page
        .locator("#ipp-world-canvas")
        .boundingBox();
      assert.ok(canvasBounds);
      await exerciseChartFeedbackEdits(
        driver,
        canvasBounds.width / canvasBounds.height,
      );
      await focusChart(driver, "bars");
      const state = await driver.inspect();
      const chart = state.charts.find((chart) => chart.id === "bars")!;
      const bounds = await g.page.locator("#ipp-world-canvas").boundingBox();
      assert.ok(bounds);
      const point = baselineBarPointer(state, bounds.width / bounds.height);
      const clientX = bounds.x + point.x * bounds.width;
      const clientY = bounds.y + point.y * bounds.height;
      await g.page.mouse.move(clientX, clientY);
      const hovered = await waitForCharts(
        driver,
        (current) => current.hover?.chart === "bars",
        "bars-hover",
      );
      assert.equal(hovered.hover?.rowId, 2n);
      await g.page.mouse.click(clientX, clientY);
      const selected = await waitForCharts(
        driver,
        (current) => current.selection?.chart === "bars",
        "bars-selection",
      );
      assert.equal(selected.selection?.rowId, 2n);
      assert.equal(selected.selection?.entity, chart.entity);
      assert.equal(selected.selection?.world.id, chart.world.id);
      assert.equal(selected.selection?.path.length, 1);
      await g.capture("charts-selected-row");
      await g.page.screenshot({
        path: `${scenario.evidence.directory}/charts-selected-row-page.png`,
        fullPage: true,
      });
      await driver.record("charts-pointer-row", {
        hovered,
        selected,
      });
      await focusChart(driver, "grid-bars");
      const grid = await driver.inspect();
      const gridPoint = baselineGridPointer(grid, bounds.width / bounds.height);
      const gridX = bounds.x + gridPoint.x * bounds.width;
      const gridY = bounds.y + gridPoint.y * bounds.height;
      await g.page.mouse.move(gridX, gridY);
      const gridHovered = await waitForCharts(
        driver,
        (current) => current.hover?.chart === "grid-bars",
        "grid-hover",
      );
      assert.equal(gridHovered.hover?.rowId, 6n);
      await g.page.mouse.click(gridX, gridY);
      const gridSelected = await waitForCharts(
        driver,
        (current) => current.selection?.chart === "grid-bars",
        "grid-selection",
      );
      assert.equal(gridSelected.selection?.rowId, 6n);
      assert.equal(gridSelected.selection?.world.id, grid.worldReference.id);
      assert.equal(gridSelected.selection?.path.length, 0);
      await g.capture("charts-selected-volumetric-row");
      await g.page.screenshot({
        path: `${scenario.evidence.directory}/charts-selected-volumetric-row-page.png`,
        fullPage: true,
      });
      await driver.record("charts-pointer-volumetric-row", {
        gridHovered,
        gridSelected,
      });
      const cameraBefore = gridSelected.camera.transform;
      await g.drag(
        [bounds.x + bounds.width * 0.05, bounds.y + bounds.height * 0.08],
        [bounds.x + bounds.width * 0.15, bounds.y + bounds.height * 0.15],
      );
      const free = await driver.inspect();
      assert.notDeepEqual(free.camera.transform, cameraBefore);
      assert.equal(free.focus, null);
      await g.capture("charts-pointer-free-angle");
      const rotated = await focusChart(driver, "single-row");
      const rotatedPoint = baselineSingleRowPointer(
        rotated,
        bounds.width / bounds.height,
      );
      const rotatedX = bounds.x + rotatedPoint.x * bounds.width,
        rotatedY = bounds.y + rotatedPoint.y * bounds.height;
      await g.page.mouse.move(rotatedX, rotatedY);
      const rotatedHover = await waitForCharts(
        driver,
        (current) => current.hover?.chart === "single-row",
        "rotated-spatial-hover",
      );
      assert.equal(rotatedHover.hover?.rowId, 2n);
      await g.page.mouse.click(rotatedX, rotatedY);
      const rotatedSelected = await waitForCharts(
        driver,
        (current) => current.selection?.chart === "single-row",
        "rotated-spatial-selection",
      );
      assert.equal(rotatedSelected.selection?.rowId, 2n);
      assert.equal(
        rotatedSelected.selection?.world.id,
        rotated.worldReference.id,
      );
      assert.equal(rotatedSelected.selection?.path.length, 0);
      await driver.record("ring-pointer-rotated-spatial-row", {
        rotatedHover,
        rotatedSelected,
      });
      await g.capture("ring-selected-rotated-spatial-row");
      await g.navigate("shapes");
      const departed = await g.inspect();
      assert.equal(departed.controllers?.length ?? 0, 0);
      assert.ok(
        departed.entities.every(
          (entity) => !entity.metadata.symbolicId?.startsWith("chart-"),
        ),
      );
      assert.equal((await g.call<string[]>("galleryWorlds")).length, 1);
      await g.navigate("charts");
      const reentered = await driver.inspect();
      assert.equal(reentered.charts.length, 10);
      assert.equal(reentered.hover, null);
      assert.equal(reentered.selection, null);
      assert.equal(reentered.world.controllers?.length ?? 0, 0);
      assert.equal((await g.call<string[]>("galleryWorlds")).length, 11);
      await g.capture("charts-reentered");
      assertCenterCamera(reentered);
      let centerPose = reentered.camera.transform;
      for (const offset of [0.14, 0.24, 0.34]) {
        await g.drag(
          [bounds.x + bounds.width * offset, bounds.y + bounds.height * 0.08],
          [
            bounds.x + bounds.width * (offset + 0.06),
            bounds.y + bounds.height * 0.08,
          ],
        );
        const turn = await driver.inspect();
        assertCenterCamera(turn);
        assert.notDeepEqual(turn.camera.transform, centerPose);
        assert.equal(turn.focus, null);
        centerPose = turn.camera.transform;
      }
      await driver.record("ring-pointer-center-turns", await driver.inspect());

      // Review artifacts use the application's full canvas after behavior checks finish.
      await g.page.addStyleTag({
        content: ".viewer-shell .canvas-frame { width: 100%; height: 100%; }",
      });
      await waitForCharts(
        driver,
        (state) =>
          (state.presentation?.viewport.width ?? 0) > bounds.width * 1.5,
        "review-viewport-ready",
      );
      const reviewCapture = async (label: string) => {
        await g.capture(label);
        await g.page.screenshot({
          path: `${scenario.evidence.directory}/${label}-page.png`,
          fullPage: true,
        });
      };
      await focusChart(driver, "center");
      await reviewCapture("review-ring-center");
      for (const [label, yaw] of [
        ["right", (2 * Math.PI) / 10],
        ["left", -(2 * Math.PI) / 10],
      ] as const) {
        await focusChart(driver, "center");
        await driver.action("navigate", { kind: "rotate", yaw, pitch: 0 });
        assertCenterCamera(await driver.inspect());
        await reviewCapture(`review-ring-turned-${label}`);
      }
      await focusChart(driver, "overview");
      await reviewCapture("review-ring-whole");
      for (const id of ["bars", "grid-bars"]) {
        const state = await focusChart(driver, id);
        await reviewCapture(`review-ring-focus-${id}`);
        const canvas = await g.page.locator("#ipp-world-canvas").boundingBox();
        assert.ok(canvas);
        const point =
          id === "bars"
            ? baselineBarPointer(state, canvas.width / canvas.height)
            : baselineGridPointer(state, canvas.width / canvas.height);
        const x = canvas.x + point.x * canvas.width,
          y = canvas.y + point.y * canvas.height;
        await g.page.mouse.move(x, y);
        await g.page.mouse.click(x, y);
        await waitForCharts(
          driver,
          (current) => current.selection?.chart === id,
          `review-${id}-selected`,
        );
        await reviewCapture(`review-ring-selected-${id}`);
      }
      await driver.action("clearSelection");
      await focusChart(driver, "bars");
      await driver.action("navigate", { kind: "zoom", amount: 0.3 });
      await driver.action("navigate", {
        kind: "rotate",
        yaw: 0.4,
        pitch: 0.16,
      });
      await reviewCapture("review-ring-inward-oblique");
      await focusChart(driver, "overview");
      await driver.action("navigate", {
        kind: "rotate",
        yaw: 0.25,
        pitch: 0.12,
      });
      await reviewCapture("review-ring-free-angle");
      assert.deepEqual(g.errors, []);
    },
  );
});

test("gallery streaming controls retain real windows, expire source rows and release datasets on reentry", {
  timeout: 120_000,
}, async (context) => {
  await runBrowserEnvironment(
    "gallery streaming charts",
    galleryEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      await g.navigate("charts");
      const inspect = () => g.call<StreamingChartsState>("gallerySceneState");
      const bounds = await g.page.locator("#ipp-world-canvas").boundingBox();
      assert.ok(bounds);
      const driver: StreamingChartsDriver = {
        inspect,
        action: async (name, args) => {
          if (name === "dataSource")
            return g.page
              .locator("#charts-data-source")
              .selectOption(String(args));
          if (name === "dataWindow")
            return g.page
              .locator("#charts-data-window")
              .selectOption(String(args));
          if (name === "streamPlayback") {
            if ((await inspect()).data.feed.playing !== args)
              await g.page.locator("#charts-stream-playback").click();
            return;
          }
          if (name === "select") {
            const point = args as { x: number; y: number };
            assert.ok(point.x > 0 && point.x < 1 && point.y > 0 && point.y < 1);
            return g.page.mouse.click(
              bounds.x + point.x * bounds.width,
              bounds.y + point.y * bounds.height,
            );
          }
          return g.call("gallerySceneAction", name, args);
        },
        capture: async (label) => {
          const frame = await g.capture(label);
          await g.page.screenshot({
            path: `${scenario.evidence.directory}/${label}-page.png`,
            fullPage: true,
          });
          return decodePng(Buffer.from(frame.dataUrl.split(",")[1]!, "base64"));
        },
        record: async (label, value) => {
          await scenario.evidence.writeJson(`${label}.json`, value);
          await scenario.evidence.record(label, { artifact: `${label}.json` });
        },
      };
      const result = await exerciseStreamingCharts(
        driver,
        bounds.width / bounds.height,
      );
      for (let pass = 0; pass < 2; pass++) {
        await g.navigate("shapes");
        assert.equal((await g.call<string[]>("galleryWorlds")).length, 1);
        for (const name of result.sourceNames)
          assert.equal(
            await g.call<boolean>("galleryDatasetExists", name),
            false,
            "Disposed scene releases its streaming source",
          );
        await g.navigate("charts");
        const state = await inspect();
        assert.equal(state.data.mode, "buffer");
        assert.equal(state.selection, null);
        assert.equal(state.hover, null);
        assert.equal((await g.call<string[]>("galleryWorlds")).length, 11);
        assertCenterCamera(state);
        await driver.record(`stream-reentry-${pass}`, state);
      }
      await g.capture("stream-reentered-fixed");
    },
  );
});
