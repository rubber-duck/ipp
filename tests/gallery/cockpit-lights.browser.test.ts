import test from "node:test";
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
