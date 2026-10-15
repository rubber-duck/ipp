import assert from "node:assert/strict";
import test from "node:test";
import { runBrowserEnvironment } from "../harness/browser.js";
import {
  applicationEnvironment,
  openApplication,
} from "./drivers/browser-application.js";
import { exerciseRenderScale } from "./scenarios/render-scale.js";
import type * as Fixture from "./pages/application.js";

test("cockpit resolution choices return to previous values in both skins", {
  timeout: 240_000,
}, async (context) => {
  await runBrowserEnvironment(
    "cockpit render resolution round trips",
    { ...applicationEnvironment, deviceScaleFactor: 1 },
    context.signal,
    async (environment) => {
      await environment.page.setViewportSize({ width: 960, height: 720 });
      const app = await openApplication(environment);
      await app.navigate("cockpit");
      await app.waitForOption("scene", "cockpit");
      await app.call("options", {
        carousel: 0,
        surface: "flat",
        lightsPlaying: false,
        atmospherePlaying: false,
        scanning: false,
      });
      const press = async (id: string) => {
        const point = await app.cockpitCoordinates(0, id);
        await app.page.mouse.click(point.x, point.y);
        await app.settle();
      };
      await press("cockpit-control-tab/gui");
      await exerciseRenderScale({
        async skin(skin) {
          await app.action("skin", skin);
          await app.waitForOption("skin", skin);
        },
        async select(scale) {
          await press(`cockpit-render-scale/${scale}`);
          await app.waitForOption("renderScale", scale);
        },
        async snapshot() {
          const state = await app.state();
          const geometry =
            await app.call<Awaited<ReturnType<typeof Fixture.cockpitGeometry>>>(
              "cockpitGeometry",
            );
          const selected = geometry.panels[0]!.entities.filter(
            (entity) =>
              entity.metadata.symbolicId?.startsWith("cockpit-render-scale/") &&
              entity.components.some(
                (component) =>
                  component.component === geometry.componentIds.GuiButton &&
                  component.fields.selected === true,
              ),
          ).map((entity) =>
            entity.metadata.symbolicId!.slice("cockpit-render-scale/".length),
          );
          return { scale: Number(state.options.renderScale), selected };
        },
        async capture(name) {
          const frame = await app.capture(name);
          return {
            width: frame.frame.width,
            height: frame.frame.height,
            foregroundPixels: frame.summary.foregroundPixels,
          };
        },
      });
      assert.deepEqual(app.errors, []);
    },
  );
});
