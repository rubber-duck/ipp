import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { runBrowserEnvironment } from "../harness/browser.js";
import {
  applicationEnvironment,
  openApplication,
} from "../gallery/drivers/browser-application.js";
import { cockpitTerminalScenario } from "./scenarios/cockpit.js";

for (const skin of ["glass", "cybertron"] as const)
  test(`hosted ${skin} cockpit terminal starts explicitly and receives spatial IPP input`, {
    timeout: 240000,
  }, async (context) => {
    await runBrowserEnvironment(
      `cockpit-terminal-${skin}`,
      { ...applicationEnvironment, crossOriginIsolation: false },
      context.signal,
      async (env) => {
        await env.page.setViewportSize({ width: 960, height: 640 });
        const app = await openApplication(env, {
          entryPath: "/target/gallery-site/index.html",
        });
        const font = JSON.parse(
          await readFile("target/terminal-build/terminal-font.json", "utf8"),
        );
        try {
          await cockpitTerminalScenario(
            app,
            [..."COCKPIT_READY"].map((character) => font.glyphs[character]),
            skin,
          );
        } catch (error) {
          await env.evidence
            .record(
              "terminal-state",
              await app.call("cockpitGeometry").catch((inspectionError) => ({
                inspectionError: String(inspectionError),
              })),
            )
            .catch(() => {});
          throw error;
        }
        assert.deepEqual(app.errors, []);
      },
    );
  });
