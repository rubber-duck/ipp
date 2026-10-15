import assert from "node:assert/strict";
import test from "node:test";
import { runBrowserEnvironment } from "../harness/browser.js";
import {
  applicationEnvironment,
  openApplication,
} from "./drivers/browser-application.js";
import { observeChartPrograms } from "./drivers/browser-chart-programs.js";
import { exerciseChartVisibility } from "./scenarios/gallery-chart-visibility.js";

test("unchanged chart revisits reuse real WebGL programs through the shared output", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "shared chart program reuse",
    applicationEnvironment,
    context.signal,
    async (environment) => {
      await environment.page.setViewportSize({ width: 480, height: 320 });
      const programs = observeChartPrograms(environment.page);
      const app = await openApplication(environment, {
        entryPath: "/examples/demos/index.html#charts",
      });
      await app.waitForOption("scene", "charts");
      await exerciseChartVisibility({
        action: (name, args) => app.action(name, args),
        programs,
        record: (name, value) => environment.evidence.record(name, value),
        async capture(name) {
          await app.capture(name);
          return environment.page.evaluate(async (helper) => {
            const fixture = await import(helper);
            return fixture.image();
          }, `${environment.urls.origin}/target/gallery-fixtures/application-browser-helper.js`);
        },
      });
      assert.deepEqual(app.errors, []);
      await environment.page.evaluate(async () => {
        await window.ippDemoApplication?.dispose();
      });
    },
  );
});
