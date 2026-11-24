import assert from "node:assert/strict";
import test from "node:test";
import { runBrowserEnvironment } from "../harness/browser.js";
import {
  applicationEnvironment,
  openApplication,
} from "./drivers/browser-application.js";
import { assertSeatedYaw, cockpitCamera } from "./scenarios/cockpit-camera.js";
import { cockpitFields } from "./cockpit-oracle.js";

test("cockpit background drag looks around Y while GUI retains its gestures", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "cockpit seated yaw",
    { ...applicationEnvironment, hasTouch: true },
    context.signal,
    async (environment) => {
      const { page } = environment;
      await page.setViewportSize({ width: 1600, height: 900 });
      const app = await openApplication(environment, {
        entryPath: "/examples/demos/index.html#cockpit",
      });
      await app.call("options", {
        lightsPlaying: false,
        atmospherePlaying: false,
        scanning: false,
      });
      type Geometry = Awaited<
        ReturnType<typeof import("./pages/application.js").cockpitGeometry>
      >;
      const camera = async () =>
        cockpitCamera(await app.call<Geometry>("cockpitGeometry"));
      const before = await camera();
      await app.capture("cockpit-camera-forward");
      await app.drag([80, 600], [320, 675]);
      const turned = await camera();
      assertSeatedYaw(before, turned);
      await app.capture("cockpit-camera-turned");
      const difference = await app.difference(
        "cockpit-camera-forward",
        "cockpit-camera-turned",
      );
      assert.ok(
        difference.changedPixels > 1600 * 900 * 0.1,
        "real drag changes the visible bridge",
      );
      await app.drag([30, 870], [30, 790]);
      await page.mouse.move(30, 870);
      await page.mouse.wheel(0, 300);
      await app.settle();
      assert.deepEqual(
        await camera(),
        turned,
        "vertical drag and wheel leave camera pose and projection fixed",
      );
      const sideHeader = await app.cockpitCoordinates(2, "cockpit-header-2");
      const focus = Number((await app.state()).options.carousel);
      await app.drag(
        [sideHeader.x, sideHeader.y],
        [sideHeader.x - 160, sideHeader.y],
      );
      assert.ok(
        Number((await app.state()).options.carousel) > focus + 0.05,
        "dragging a side pane moves the carousel after looking right",
      );
      assert.deepEqual(await camera(), turned);
      await app.action("resetCamera");
      await app.call("options", { carousel: 0 });
      assert.deepEqual(await camera(), before);
      const header = await app.cockpitCoordinates(
        0,
        "cockpit-header-0",
        0.5,
        0.5,
      );
      await app.drag([header.x, header.y], [header.x - 100, header.y]);
      assert.deepEqual(
        await camera(),
        before,
        "dragging a pane header does not rotate the camera",
      );
      await app.capture("cockpit-camera-gui-drag");
      await app.call("options", { carousel: 0, illumination: 0.75 });
      const touch = await page.context().newCDPSession(page);
      type Point = { x: number; y: number };
      const touchDrag = async (from: Point, to: Point) => {
        await touch.send("Input.dispatchTouchEvent", {
          type: "touchStart",
          touchPoints: [from],
        });
        await touch.send("Input.dispatchTouchEvent", {
          type: "touchMove",
          touchPoints: [to],
        });
        await touch.send("Input.dispatchTouchEvent", {
          type: "touchEnd",
          touchPoints: [],
        });
        await app.settle();
      };
      try {
        const slider = "cockpit-illumination/slider";
        await touchDrag(
          await app.cockpitCoordinates(0, slider, 0.5, 0.8),
          await app.cockpitCoordinates(0, slider, 0.78, 0.8),
        );
        assert.ok(
          Number((await app.state()).options.illumination) > 0.9,
          "the lower edge of the thumb accepts a drag away from the rail",
        );
        const scroll = async () => {
          const geometry = await app.call<Geometry>("cockpitGeometry");
          const fields = (kind: string) =>
            cockpitFields(
              geometry.panels[0]!,
              "cockpit-content-0",
              geometry.componentIds[kind]!,
            );
          return {
            bar: fields("GuiScrollView"),
            width: Number(fields("CanvasBounds").width),
          };
        };
        const { bar, width } = await scroll();
        assert.ok(Number(bar.capacity_y) > 0);
        assert.ok(Number(bar.bar_thickness) >= 24);
        const x =
          1 - (Number(bar.bar_inset) + Number(bar.bar_thickness) * 0.8) / width;
        await touchDrag(
          await app.cockpitCoordinates(0, "cockpit-content-0", x, 0.35),
          await app.cockpitCoordinates(0, "cockpit-content-0", x, 0.65),
        );
        assert.ok(
          Number((await scroll()).bar.offset_y) > 0,
          "dragging near the inner edge of the scrollbar thumb scrolls down",
        );
        const border = await app.cockpitCoordinates(0, "cockpit-grip-0", 0.99);
        await touchDrag(border, { x: border.x - 260, y: border.y });
        assert.ok(
          Number((await app.state()).options.carousel) > 0.05,
          "dragging the side border with a finger moves the pane",
        );
        assert.deepEqual(await camera(), before);
      } finally {
        await touch.detach();
      }
      assert.deepEqual(app.errors, []);
    },
  );
});
