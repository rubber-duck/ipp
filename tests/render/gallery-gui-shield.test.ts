/** Shield graphics and physical input survive scene/settings visibility changes. */
import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";
import { openGallery } from "./gallery-driver.js";
import { projectContent } from "./gallery-gui-panel.js";
import {
  dynamicProperty,
  fieldsWith,
  sceneEntity,
} from "./gallery-gui-support.js";
import {
  environment,
  enterWorkspace,
  waitApp,
  spacing,
  waitSpacing,
} from "./gallery-scanner-support.js";

test("Gallery GUI restores shield graphics through visibility and interlock cycles", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "Scanner shield restoration",
    environment("shield"),
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario, {
        initialPage: "gui",
        canvasShare: 1,
      });
      await enterWorkspace(g);
      await g.call("gallerySceneAction", "setAutoscan", false);
      await g.call("gallerySceneAction", "setReducedMotion", true);
      // The same shield survives visibility and material changes. Reset only
      // here makes the front-cap pixel oracle independent of manual viewing.
      await g.call("gallerySceneAction", "setExploded", false);
      await waitSpacing(g, 0);
      await g.page.locator("#reset-camera").click();
      const shieldPixels = async (label: string) => {
        const proof = await g.capture(label);
        assert.equal(proof.frame.failedDrawCalls, 0);
        const points = await projectContent(
          g,
          [
            [804.75, 533.75],
            [960.75, 533.75],
            [804.75, 601.75],
            [960.75, 601.75],
          ],
          3 * (await spacing(g)),
        );
        const region = await g.call<{
          pixels: string;
          width: number;
          height: number;
        }>("viewerCaptureRegionPixels", label, [
          Math.min(...points.map((p) => p.x)),
          Math.min(...points.map((p) => p.y)),
          Math.max(...points.map((p) => p.x)),
          Math.max(...points.map((p) => p.y)),
        ]);
        const pixels = Buffer.from(region.pixels, "base64");
        let amber = 0;
        for (let at = 0; at < pixels.length; at += 4)
          if (
            pixels[at]! > 150 &&
            pixels[at + 1]! > 80 &&
            pixels[at + 2]! < 110
          )
            amber++;
        await scenario.evidence.record(label, {
          amber,
          area: region.width * region.height,
        });
        return amber;
      };
      const shieldIdentity = sceneEntity(
        await g.inspect(),
        "gui-input-shield",
      ).id;
      for (const [shape, facing, cache] of [
        ["cylinder", "outside", "cached"],
        ["sphere", "inside", "direct"],
        ["flat", "outside", "automatic"],
      ] as const) {
        await g.call("gallerySceneAction", "app", {
          method: "settings",
          args: [true],
        });
        await waitApp(g, (v) => v.state.app.settings);
        await g.call("gallerySceneAction", "selectSurfaceShape", shape);
        await g.call("gallerySceneAction", "selectSurfaceFacing", facing);
        await g.call("gallerySceneAction", "selectSurfaceCache", cache);
        await g.call("gallerySceneAction", "setVectorOnly", true);
        await waitApp(g, (v) => v.state.vectorOnly);
        await g.call("gallerySceneAction", "setVectorOnly", false);
        await waitApp(g, (v) => !v.state.vectorOnly);
        await g.call("gallerySceneAction", "app", {
          method: "settings",
          args: [false],
        });
        await waitApp(
          g,
          (v) => !v.state.app.settings && v.state.shieldBlocker !== undefined,
        );
        assert.equal(
          sceneEntity(await g.inspect(), "gui-input-shield").id,
          shieldIdentity,
        );
        await g.call("gallerySceneAction", "setExploded", true);
        await g.call("gallerySceneAction", "setLayerStep", 0.15);
        await waitSpacing(g, 0.15);
        await g.page.locator("#reset-camera").click();
        await g.call("gallerySceneAction", "setShieldArmed", true);
        const armed = await shieldPixels(
          `scanner-shield-${shape}-${facing}-armed`,
        );
        assert.ok(
          armed > 20,
          `${shape}/${facing}: armed graphic did not return`,
        );
        await g.call("gallerySceneAction", "setShieldArmed", false);
        const lifted = await shieldPixels(
          `scanner-shield-${shape}-${facing}-lifted`,
        );
        assert.ok(
          armed > lifted * 3 + 5,
          `${shape}/${facing}: interlock changed state without repainting the graphic`,
        );
        await g.call("gallerySceneAction", "setShieldArmed", true);
      }
      await g.call("gallerySceneAction", "setShieldArmed", false);
      await waitApp(g, (v) => !v.state.shieldArmed);
      await g.capture("scanner-shield-lifted-after-cycles");
      await g.call("gallerySceneAction", "setShieldArmed", true);
      await waitApp(
        g,
        (v) => v.state.shieldArmed && v.state.shieldBlocker !== undefined,
      );
      const restored = await g.inspect();
      const shield = sceneEntity(restored, "gui-input-shield");
      assert.equal(shield.id, shieldIdentity);
      const shieldPose = fieldsWith(restored, "gui-input-shield", "sx");
      for (const axis of ["sx", "sy", "sz"])
        assert.ok(Number(shieldPose[axis]) > 0);
      assert.equal(
        dynamicProperty(restored, "gui-input-shield", "visible").value,
        1,
      );
      const proof = await g.capture("scanner-shield-restored-after-cycles");
      assert.equal(proof.frame.failedDrawCalls, 0);
      const amber = await shieldPixels("scanner-shield-restored-after-cycles");
      assert.ok(
        amber > 20,
        "shield graphic did not return after visibility/interlock cycles",
      );
      await g.page.screenshot({
        path: resolve(
          scenario.evidence.directory,
          "scanner-shield-restored-page.png",
        ),
        fullPage: true,
      });
      await g.call("gallerySceneAction", "setExploded", false);
      await waitSpacing(g, 0);
      await g.page.locator("#reset-camera").click();
      await g.call("gallerySceneAction", "app", { method: "logout", args: [] });
      await waitApp(g, (v) => v.state.app.phase === "login");
      assert.equal(
        dynamicProperty(await g.inspect(), "gui-input-shield", "visible").value,
        0,
      );
      await enterWorkspace(g);
      await waitApp(g, (v) => v.state.shieldBlocker !== undefined);
      assert.equal(
        sceneEntity(await g.inspect(), "gui-input-shield").id,
        shieldIdentity,
      );
      await g.call("gallerySceneAction", "setShieldArmed", false);
      await shieldPixels("scanner-shield-login-cycle-lifted");
      await g.call("gallerySceneAction", "setShieldArmed", true);
      assert.ok((await shieldPixels("scanner-shield-login-cycle-armed")) > 20);
      await g.call("observeGalleryGuiInput");
      const [pulsePoint] = await projectContent(g, [[882.75, 567.75]]);
      await g.page.mouse.click(pulsePoint!.clientX, pulsePoint!.clientY);
      await g.capture("scanner-shield-restored-blocking");
      const outcomes = await g.call<{ outcomes: { disposition: string }[] }>(
        "finishGalleryGuiInputObservation",
      );
      assert.ok(outcomes.outcomes.some((o) => o.disposition === "blocked"));
      assert.deepEqual(g.errors, []);
    },
  );
});
