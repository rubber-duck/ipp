import assert from "node:assert/strict";
import test from "node:test";
import { runBrowserEnvironment } from "../harness/browser.js";
import {
  applicationEnvironment,
  openApplication,
} from "./drivers/browser-application.js";
import { exerciseRenderScale } from "./scenarios/render-scale.js";
import { cockpitFields } from "./cockpit-oracle.js";
import type * as Fixture from "./pages/application.js";

test("portrait toolbar fits the Display label and opens its resolution choices", {
  timeout: 120_000,
}, async (context) => {
  await runBrowserEnvironment(
    "portrait Display menu",
    { ...applicationEnvironment, deviceScaleFactor: 3, hasTouch: true },
    context.signal,
    async (environment) => {
      const { page } = environment;
      await page.setViewportSize({ width: 393, height: 695 });
      const app = await openApplication(environment, {
        entryPath: "/examples/demos/index.html#cockpit",
      });
      const { shell } = await app.state();
      const bounds = await page.evaluate(
        () =>
          window.ippDemoApplication!.sceneClient!.components.CanvasBounds!.id,
      );
      const rect = (id: string) => cockpitFields(shell, id, bounds);
      const trigger = rect("demo-display");
      const label = rect("demo-display/label");
      const caret = rect("demo-display/caret");
      assert.ok(
        Number(label.width) > Number(label.height) && Number(label.height) > 0,
        "Display fits on one line",
      );
      assert.ok(
        Number(caret.width) > 0 &&
          Number(caret.x) >= Number(label.x) + Number(label.width) - 1,
      );
      assert.ok(
        Number(caret.x) + Number(caret.width) <=
          Number(trigger.x) + Number(trigger.width) + 1,
      );
      const tap = async (id: string) => {
        const point = await app.coordinates(id);
        assert.ok(
          point.x > 0 && point.x < 393 && point.y > 0 && point.y < 695,
          id,
        );
        await page.touchscreen.tap(point.x, point.y);
        await app.settle();
      };
      await app.capture("portrait-display-closed");
      await tap("demo-display");
      await tap("demo-render-scale");
      await app.capture("portrait-display-open");
      assert.ok(
        (
          await app.difference(
            "portrait-display-closed",
            "portrait-display-open",
          )
        ).changedPixels > 1000,
      );
      await tap("demo-render-scale/options/0.75");
      await app.waitForOption("renderScale", 0.75);
      assert.deepEqual(app.errors, []);
    },
  );
});

test("cockpit resolution choices return to previous values in both skins", {
  timeout: 240_000,
}, async (context) => {
  await runBrowserEnvironment(
    "cockpit render resolution round trips",
    { ...applicationEnvironment, deviceScaleFactor: 1, hasTouch: true },
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
        await app.page.touchscreen.tap(point.x, point.y);
        await app.settle();
      };
      await press("cockpit-control-tab/gui");
      await press("cockpit-control-tab/scene");
      await app.cockpitCoordinates(0, "cockpit-illumination/slider");
      await press("cockpit-control-tab/gui");
      await press("cockpit-next");
      await app.waitForOption("carousel", 1);
      // Keep the first pane in view while its Previous button is enabled.
      await app.call("options", { carousel: 0.25 });
      await press("cockpit-previous");
      await app.waitForOption("carousel", 0);
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
