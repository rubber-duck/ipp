import assert from "node:assert/strict";
import test from "node:test";
import type {
  GeometryPickResultEvent,
  CameraProjectResultEvent,
} from "@ipp/client";
import { runBrowserEnvironment } from "../browser/environment.js";
import {
  openGallery,
  galleryEnvironment,
  position,
  transform,
  selected,
} from "./gallery-driver.js";

const distance = (a: number[], b: number[]) =>
  Math.hypot(...a.map((value, index) => value - b[index]!));

test("combined objects drag on the picked view plane and reject late or canceled projection results", {
  timeout: 90_000,
}, async (context) => {
  await runBrowserEnvironment(
    "combined object dragging",
    { ...galleryEnvironment, deviceScaleFactor: 2 },
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      await g.navigate("lighting");
      await g.seek(0);
      const camera = transform(await g.inspect());
      for (const id of [
        "lighting-cube",
        "lighting-sphere",
        "lighting-pillar",
        "lighting-spot",
        "lighting-point",
        "lighting-fill",
        "lighting-skinning",
      ]) {
        const before = await g.capture(`${id}-before-drag`);
        const start = await g.locate(id);
        const bounds = await g.page.locator("#ipp-world-canvas").boundingBox();
        assert.ok(bounds);
        const dx = 0.045,
          dy = -0.025;
        const projection = await g.call<CameraProjectResultEvent>(
          "cameraQuery",
          {
            type: "CameraProjectQuery",
            x: start.x + dx,
            y: start.y + dy,
            ...start.viewport,
            plane: start.hit.viewPlane,
          },
        );
        assert.ok(projection.ok && projection.position);
        const expected = position(before.inspection, id).map(
          (value, axis) =>
            value + projection.position![axis]! - start.hit.position[axis]!,
        );
        await g.drag(
          [start.clientX, start.clientY],
          [
            start.clientX + bounds.width * dx,
            start.clientY + bounds.height * dy,
          ],
        );
        const after = await g.capture(`${id}-after-drag`);
        assert.ok(
          distance(position(after.inspection, id), expected) < 0.02,
          `${id} preserves grab offset`,
        );
        assert.deepEqual(transform(after.inspection), camera);
        assert.deepEqual(selected(after.inspection), [id]);
        assert.ok(
          (await g.difference(`${id}-before-drag`, `${id}-after-drag`))
            .changedPixels > 15,
        );
        const point = await g.locate(id);
        const repick = await g.call<GeometryPickResultEvent>("cameraQuery", {
          type: "GeometryPickQuery",
          x: point.x,
          y: point.y,
          ...point.viewport,
        });
        assert.ok(repick.ok && repick.hit?.entity === start.hit.entity);
      }
      const id = "lighting-sphere";
      let point = await g.locate(id);
      await g.call("delayNextCameraQuery", "GeometryPickQuery");
      try {
        const before = position(await g.inspect(), id);
        await g.drag(
          [point.clientX, point.clientY],
          [point.clientX + 40, point.clientY - 10],
        );
        await g.held();
        assert.deepEqual(position(await g.inspect(), id), before);
        await g.call("releaseQuery");
        const buffered = await g.capture("buffered-hit-drag");
        assert.ok(distance(position(buffered.inspection, id), before) > 0.1);

        for (const cancellation of [
          "blur",
          "wheel",
          "reset",
          "page",
        ] as const) {
          point = await g.locate(id);
          const before = position(await g.inspect(), id);
          await g.call("delayNextCameraQuery", "CameraProjectQuery");
          await g.drag(
            [point.clientX, point.clientY],
            [point.clientX + 30, point.clientY],
          );
          await g.held();
          if (cancellation === "blur")
            await g.page.evaluate(() =>
              window.dispatchEvent(new Event("blur")),
            );
          else if (cancellation === "wheel") await g.page.mouse.wheel(0, 50);
          else if (cancellation === "reset")
            await g.page.locator("#reset-camera").click();
          else await g.navigate("shapes");
          await g.call("releaseQuery");
          await g.settle();
          if (cancellation === "page") {
            await g.navigate("lighting");
            await g.seek(0);
          }
          assert.deepEqual(
            position(
              (await g.capture(`canceled-${cancellation}`)).inspection,
              id,
            ),
            before,
          );
        }
      } finally {
        await g.call("releaseQuery");
      }

      // A running light is held during the grab and resumes its own clock afterward.
      await g.page.locator("#animation-target").selectOption("lighting-point");
      // Slow the small moving marker so the probe-to-pointer transport delay stays
      // inside its hit area; the runtime clock still advances normally.
      await g.page.locator("#animation-speed").selectOption("0.25");
      await g.page.locator("#animation-play").click();
      const light = await g.locate("lighting-point");
      await g.page.mouse.move(light.clientX, light.clientY);
      await g.page.mouse.down();
      await g.waitFor((snapshot) => {
        const entity = snapshot.entities.find(
          (entity) => entity.metadata.symbolicId === "lighting-point",
        );
        return (
          snapshot.controllers?.some(
            (player) =>
              player.description.drivers.some(
                (driver) => driver.target === entity?.id,
              ) && player.state === "paused",
          ) === true
        );
      });
      await g.page.mouse.move(light.clientX + 35, light.clientY, { steps: 4 });
      await g.page.mouse.up();
      await g.settle();
      await g.waitFor(
        (inspection) =>
          inspection.controllers?.some(
            (player) => player.state === "playing",
          ) === true,
      );
      assert.deepEqual(g.errors, []);
    },
  );
});
