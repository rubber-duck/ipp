import test from "node:test";
import { runBrowserEnvironment } from "../harness/browser.js";
import {
  applicationEnvironment,
  openApplication,
} from "./drivers/browser-application.js";
import { exercisePlatformerStartup } from "./scenarios/platformer-startup.js";
import { exercisePlatformerGaitSelection } from "./scenarios/platformer-controls.js";
import { browserPlatformerControls } from "./drivers/browser-platformer.js";

test("platformer browser startup prepares pose origins before playback", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "platformer startup",
    applicationEnvironment,
    context.signal,
    async (environment) => {
      const app = await openApplication(environment, {
        entryPath: "/examples/demos/index.html#platformer",
      });
      await exercisePlatformerStartup(app);
      await app.action("playback", false);
      await app.action("reset");
      for (const skin of ["glass", "cybertron"]) {
        await app.action("skin", skin);
        await exercisePlatformerGaitSelection(browserPlatformerControls(app));
        await app.capture(`platformer-repeated-gait-clicks-${skin}`);
      }
    },
  );
});
