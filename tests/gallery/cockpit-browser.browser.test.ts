import assert from "node:assert/strict";
import test from "node:test";
import { runBrowserEnvironment } from "../harness/browser.js";
import {
  applicationEnvironment,
  openApplication,
} from "./drivers/browser-application.js";
import { cockpitBrowserDriver } from "./drivers/browser-cockpit-browser.js";
import { cockpitBrowserScenario } from "./scenarios/cockpit-browser.js";

test("cockpit HTML forms and history receive real Surface input at 2x display scale", {
  timeout: 360_000,
}, async (context) => {
  await runBrowserEnvironment(
    "cockpit-browser",
    {
      ...applicationEnvironment,
      operationTimeoutMs: 90_000,
      deviceScaleFactor: 2,
    },
    context.signal,
    async (env) => {
      env.page.setDefaultTimeout(90_000);
      // Load the large cockpit textures with less software-rendering contention.
      // All interaction and pixel assertions use the full viewport below.
      await env.page.setViewportSize({ width: 320, height: 240 });
      const app = await openApplication(env, {
        entryPath: "/target/gallery-site/index.html#cockpit",
      });
      await env.page.setViewportSize({ width: 800, height: 600 });
      await env.page.waitForFunction(
        () =>
          window.ippDemoCanvas?.viewport?.width ===
          800 * window.devicePixelRatio,
      );
      await app.settle();
      await app.action("carousel", 3);
      await env.page.waitForTimeout(350);
      const result = await cockpitBrowserScenario(
        cockpitBrowserDriver(app),
      ).catch(async (error) => {
        await app.capture("failure").catch(() => {});
        throw error;
      });
      await env.evidence.record("html-pages", result);
      assert.deepEqual(app.errors, []);
    },
  );
});

for (const mode of [
  "flag-disabled",
  "geometry-disabled",
  "other-browser",
] as const)
  test(`cockpit HTML ${mode} explains availability`, {
    timeout: 180_000,
  }, async (context) => {
    await runBrowserEnvironment(
      `cockpit-browser-${mode}`,
      {
        ...applicationEnvironment,
        operationTimeoutMs: 90_000,
        htmlInCanvas: mode === "geometry-disabled",
        experimentalWebPlatformFeatures: false,
        ...(mode === "other-browser"
          ? { userAgent: "Mozilla/5.0 Firefox/145.0" }
          : {}),
      },
      context.signal,
      async (env) => {
        env.page.setDefaultTimeout(90_000);
        await env.page.setViewportSize({ width: 320, height: 240 });
        const app = await openApplication(env, {
          entryPath: "/examples/demos/index.html#cockpit",
        });
        await env.page.setViewportSize({ width: 960, height: 640 });
        await env.page.waitForFunction(
          () => window.ippDemoCanvas?.viewport?.width === 960,
        );
        await app.settle();
        await app.action("carousel", 3);
        await env.page.waitForTimeout(350);
        const driver = cockpitBrowserDriver(app);
        const state = await driver.state();
        assert.match(
          state.status,
          mode === "other-browser"
            ? /Not supported outside of Chrome/
            : /Enable both Chrome flags/,
        );
        assert.equal(state.domSources, 0);
        if (mode === "geometry-disabled")
          assert.equal(
            await env.page.evaluate(
              () => "updateElementGeometry" in HTMLCanvasElement.prototype,
            ),
            true,
            "the incomplete flag configuration exposes the API",
          );
        if (mode !== "other-browser") {
          await env.page
            .context()
            .grantPermissions(["clipboard-read", "clipboard-write"]);
          for (const [button, address] of [
            ["copy", "chrome://flags/#canvas-draw-element"],
            [
              "copy-experimental",
              "chrome://flags/#enable-experimental-web-platform-features",
            ],
          ] as const) {
            await driver.press(button);
            let copied = "";
            const deadline = performance.now() + 10_000;
            while (performance.now() < deadline) {
              copied = await env.page.evaluate(() =>
                navigator.clipboard.readText(),
              );
              if (copied === address) break;
              await env.page.waitForTimeout(50);
            }
            assert.equal(copied, address);
          }
        }
        await app.capture(mode);
        assert.deepEqual(app.errors, []);
      },
    );
  });
