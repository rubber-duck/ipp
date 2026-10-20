import test from "node:test";
import { runBrowserEnvironment } from "../harness/browser.js";
import { invoke } from "../harness/page-calls.js";
import {
  applicationEnvironment,
  openApplication,
} from "./drivers/browser-application.js";
import { exerciseFrameRateHistory } from "./scenarios/frame-rate-history.js";
import type * as Fixture from "./pages/frame-rate.js";

test("cockpit full FPS history stays readable without per-sample draws", {
  // Both surfaces render full history at DPR 2, including software browsers.
  timeout: 360_000,
}, async (context) => {
  await runBrowserEnvironment(
    "cockpit dense FPS history",
    { ...applicationEnvironment, deviceScaleFactor: 2 },
    context.signal,
    async (environment) => {
      await environment.page.setViewportSize({ width: 1224, height: 756 });
      const app = await openApplication(environment);
      await app.navigate("cockpit");
      await app.call("options", { surface: "flat", carousel: 1 });
      const helper = `${environment.urls.origin}/target/gallery-fixtures/frame-rate-browser-helper.js`;
      const call = (name: string, ...args: unknown[]) =>
        invoke<Awaited<ReturnType<typeof Fixture.append>>>(
          app.page,
          helper,
          name,
          args,
        );
      await exerciseFrameRateHistory({
        app,
        call,
        idle: (milliseconds) => app.page.waitForTimeout(milliseconds),
        record: (name, value) => environment.evidence.record(name, value),
      });
    },
  );
});
