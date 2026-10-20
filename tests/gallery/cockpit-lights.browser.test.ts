import test from "node:test";
import { cockpitFields, assertCylinder } from "./cockpit-oracle.js";
import assert from "node:assert/strict";
import { runBrowserEnvironment } from "../harness/browser.js";
import {
  applicationEnvironment,
  openApplication,
} from "./drivers/browser-application.js";
import { exerciseCockpitLights } from "./scenarios/cockpit-lights.js";
import type { RgbaImage } from "../../tools/shared-host/images.js";

test("cockpit light masks visibly animate and pause in the browser", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "cockpit light motion",
    applicationEnvironment,
    context.signal,
    async (environment) => {
      await environment.page.setViewportSize({ width: 1600, height: 900 });
      const app = await openApplication(environment, {
        entryPath: "/examples/demos/index.html#cockpit",
      });
      const result = await exerciseCockpitLights({
        state: app.state,
        action: app.action,
        async cameraViewport() {
          type Geometry = Awaited<
            ReturnType<typeof import("./pages/application.js").cockpitGeometry>
          >;
          const { cameraSurface, viewport } =
            await app.call<Geometry>("cockpitGeometry");
          const scaleX = viewport.devicePixelRatio / viewport.width;
          const scaleY = viewport.devicePixelRatio / viewport.height;
          return [
            cameraSurface.x * scaleX,
            cameraSurface.y * scaleY,
            cameraSurface.width * scaleX,
            cameraSurface.height * scaleY,
          ] as const;
        },
        async advance(seconds) {
          const initial = await app.inspect();
          await app.waitFor((state) => state.time >= initial.time + seconds);
        },
        async capture(name) {
          await app.capture(name);
          return app.call<RgbaImage>("image");
        },
      });
      await environment.evidence.record("light-motion", result);
      assert.deepEqual(app.errors, []);
    },
  );
});

test("cockpit bridge and GUI mount retain their world geometry across browser aspect changes", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "cockpit spatial resize",
    applicationEnvironment,
    context.signal,
    async (environment) => {
      await environment.page.setViewportSize({ width: 1600, height: 900 });
      const app = await openApplication(environment, {
        entryPath: "/examples/demos/index.html#cockpit",
      });
      await app.call("options", {
        atmospherePlaying: false,
        lightsPlaying: false,
        scanning: false,
      });
      type Geometry = Awaited<
        ReturnType<typeof import("./pages/application.js").cockpitGeometry>
      >;
      const geometry = () => app.call<Geometry>("cockpitGeometry");
      const poses = (state: Geometry) =>
        [
          "cockpit-bridge",
          "cockpit-console-shell",
          "cockpit-carousel-mount",
          "cockpit-carousel",
          "cockpit-screen-0",
        ].map((name) =>
          cockpitFields(state.scene, name, state.componentIds.Transform!),
        );
      const initial = poses(await geometry());
      for (const [width, height] of [
        [1600, 900],
        [2560, 800],
        [800, 1200],
      ]) {
        await environment.page.setViewportSize({
          width: width!,
          height: height!,
        });
        await environment.page.waitForTimeout(150);
        await app.settle();
        const state = await geometry();
        assert.deepEqual(
          poses(state),
          initial,
          "viewport resize cannot change physical bridge or cylinder geometry",
        );
        assertCylinder(state.scene, state.componentIds, 8.8);
        await app.capture(`cockpit-spatial-${width}x${height}`);
        const frame = await app.call<RgbaImage>("image");
        assert.equal(frame.width, width);
        assert.equal(frame.height, height);
        let opaqueDetail = 0;
        for (let i = 0; i < frame.pixels.length; i += 4)
          if (
            frame.pixels[i]! > 90 &&
            Math.abs(frame.pixels[i]! - frame.pixels[i + 2]!) < 40
          )
            opaqueDetail++;
        assert.ok(
          opaqueDetail > width! * height! * 0.02,
          "completed frame contains the surrounding cabin and console",
        );
      }
      assert.deepEqual(app.errors, []);
    },
  );
});
