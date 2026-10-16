import assert from "node:assert/strict";
import type { openApplication } from "./browser-application.js";
import type * as Fixture from "../pages/application.js";
import type { PlatformerControlsDriver } from "../scenarios/platformer-controls.js";

/** Locate real projected hits, then click through the browser input adapter. */
export function browserPlatformerControls(
  app: Awaited<ReturnType<typeof openApplication>>,
): Pick<PlatformerControlsDriver, "state" | "press" | "advance"> {
  const points = new Map<string, readonly [number, number]>();
  const state = () =>
    app.call<Awaited<ReturnType<typeof Fixture.platformerControls>>>(
      "platformerControls",
    );
  return {
    state,
    async press(id) {
      const bounds = await app.page.locator("#ipp-demo-canvas").boundingBox();
      assert.ok(bounds);
      const move = async (point: readonly [number, number]) => {
        await app.page.mouse.move(
          bounds.x + point[0] * bounds.width,
          bounds.y + point[1] * bounds.height,
        );
        const snapshot = await state();
        for (const hovered of snapshot.hovered)
          if (hovered) points.set(hovered, point);
        return snapshot.hovered.includes(id);
      };
      if (points.has(id) && !(await move(points.get(id)!))) points.delete(id);
      if (!points.has(id)) {
        search: for (let y = 0.14; y <= 0.7; y += 0.025) {
          for (let x = 0.25; x <= 0.76; x += 0.025) {
            if (await move([x, y])) break search;
          }
        }
      }
      const point = points.get(id);
      assert.ok(point, `actual browser input finds ${id}`);
      await app.page.mouse.click(
        bounds.x + point[0] * bounds.width,
        bounds.y + point[1] * bounds.height,
      );
      await app.settle();
    },
    async advance(seconds) {
      const initial = await app.inspect();
      await app.waitFor((snapshot) => snapshot.time >= initial.time + seconds);
    },
  };
}
