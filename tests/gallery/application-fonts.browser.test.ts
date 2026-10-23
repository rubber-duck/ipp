import assert from "node:assert/strict";
import test from "node:test";
import { runBrowserEnvironment } from "../harness/browser.js";
import {
  applicationEnvironment,
  openApplication,
} from "./drivers/browser-application.js";
import { exerciseApplicationFonts } from "./scenarios/application-fonts.js";
import type { ApplicationFontsState } from "./support/application-fonts.js";

test("browser application switches fonts independently of themes", {
  timeout: 120_000,
}, async (context) => {
  await runBrowserEnvironment(
    "application fonts",
    applicationEnvironment,
    context.signal,
    async (environment) => {
      await environment.page.setViewportSize({ width: 960, height: 640 });
      const app = await openApplication(environment);
      await exerciseApplicationFonts({
        ...app,
        fonts: () => app.call<ApplicationFontsState>("fonts"),
        press: async (...ids) => {
          for (const id of ids) await app.press(id);
        },
        changedPixels: async (before, after) =>
          (await app.difference(before, after)).changedPixels,
      });
      assert.deepEqual(app.errors, []);
    },
  );
});
