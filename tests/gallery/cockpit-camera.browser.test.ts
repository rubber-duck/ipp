import assert from "node:assert/strict";
import test from "node:test";
import { runBrowserEnvironment } from "../harness/browser.js";
import {
  applicationEnvironment,
  openApplication,
} from "./drivers/browser-application.js";
import { assertSeatedYaw, cockpitCamera } from "./scenarios/cockpit-camera.js";

test("cockpit background drag looks around Y while GUI retains its gestures", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "cockpit seated yaw",
    applicationEnvironment,
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
      assert.deepEqual(app.errors, []);
    },
  );
});
