import test from "node:test";
import { runBrowserEnvironment } from "../harness/browser.js";
import {
  applicationEnvironment,
  openApplication,
} from "./drivers/browser-application.js";
import { exerciseSurfaceQuality } from "./scenarios/surface-quality.js";

test("cockpit scrolling and cache controls retain fullscreen scene detail", {
  // Six cache/motion comparisons capture the full 2448 x 1512 output. Software
  // rendering needs a larger total budget; input/state waits stay bounded.
  timeout: 600_000,
}, async (context) => {
  await runBrowserEnvironment(
    "cockpit scene density",
    { ...applicationEnvironment, deviceScaleFactor: 2 },
    context.signal,
    async (environment) => {
      await environment.page.setViewportSize({ width: 1224, height: 756 });
      const app = await openApplication(environment);
      await exerciseSurfaceQuality(app, (name, data) =>
        environment.evidence.record(name, data),
      );
    },
  );
});
