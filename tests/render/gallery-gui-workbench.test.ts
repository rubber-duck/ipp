/** Scanner settings exercise real scene authoring, curved layers and recovery. */
import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import { writeDataUrl } from "./evidence.js";
import { runBrowserEnvironment } from "../browser/environment.js";
import { openGallery } from "./gallery-driver.js";
import { projectContent } from "./gallery-gui-panel.js";
import {
  dynamicProperty,
  fieldsWith,
  guiApplication,
  sceneEntity,
  waitForGuiState,
} from "./gallery-gui-support.js";
import {
  environment,
  enterWorkspace,
  find,
  press,
  waitApp,
  openSettings,
  selectSettingsPage,
  selectPresentationPage,
  dropdown,
  spacing,
  waitSpacing,
} from "./gallery-scanner-support.js";

test("Gallery GUI keeps stage selections valid and corrects rejected scene edits", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "Scanner declaration recovery",
    environment("recovery"),
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario, {
        initialPage: "gui",
        canvasShare: 1,
      });
      await enterWorkspace(g);
      await g.call("gallerySceneAction", "setAutoscan", false);
      await g.call("gallerySceneAction", "setReducedMotion", true);
      await g.call("gallerySceneAction", "setExploded", true);
      await waitSpacing(g, 0.9);
      await openSettings(g, "scene");
      await g.call("galleryIndependentWorld", "open");
      try {
        const initial = await g.inspect();
        const identities = [
          "gui-projector-floor",
          "gui-projector-base",
          "gui-demo",
        ].map((id) => sceneEntity(initial, id).id);
        const survivor = await g.call<{ before: bigint; after: bigint }>(
          "galleryIndependentWorld",
          "observe",
        );
        await press(
          g,
          await find(g, "gui-scene-tree/row/stage/chevron"),
          4 * (await spacing(g)),
        );
        await g.capture("scanner-stage-expanded");
        // Completed bounds already include ancestor scrolling. Move the tree's
        // actual wheel chain so its lower child rows enter the viewport.
        const tree = await find(g, "gui-scene-tree");
        const [treePoint] = await projectContent(
          g,
          [
            [
              tree.bounds[0] + tree.bounds[2] / 2,
              tree.bounds[1] + tree.bounds[3] / 2,
            ],
          ],
          4 * (await spacing(g)),
        );
        await g.page.mouse.move(treePoint!.clientX, treePoint!.clientY);
        await g.page.mouse.wheel(0, 300);
        await g.capture("scanner-tree-scrolled");
        await g.call("gallerySceneAction", "tuning", {
          method: "setFocus",
          args: [],
        });
        const closeSettings = async () => {
          await press(
            g,
            await find(g, "gui-settings-close"),
            4 * (await spacing(g)),
          );
          await waitApp(g, (value) => !value.state.app.settings);
          await g.page.mouse.move(1, 1);
        };
        // Compare the same unobstructed workspace for every selection. The
        // settings tree changes focus/selection paint over the stage pixels.
        await closeSettings();
        await g.page.mouse.move(1, 1);
        await g.capture("stage-unselected");
        for (const selected of ["floor", "base", "stage"] as const) {
          await openSettings(g, "scene");
          await press(
            g,
            await find(g, `gui-scene-tree/row/${selected}`),
            4 * (await spacing(g)),
          );
          await waitApp(g, (v) => v.state.tuning.focus === selected);
          const current = await g.inspect();
          for (const part of ["floor", "base"] as const) {
            const gain = dynamicProperty(
              current,
              `gui-projector-${part}`,
              "gain",
            );
            assert.equal(gain.kind, "f32");
            assert.ok(
              Math.abs(
                Number(gain.value) -
                  (selected === part || selected === "stage" ? 1.35 : 1),
              ) < 1e-5,
            );
          }
          assert.deepEqual(
            ["gui-projector-floor", "gui-projector-base", "gui-demo"].map(
              (id) => sceneEntity(current, id).id,
            ),
            identities,
          );
          await closeSettings();
          await g.page.mouse.move(1, 1);
          const frame = await g.capture(`stage-${selected}-selected`);
          assert.equal(frame.frame.failedDrawCalls, 0);
          const pixels = await g.call<{ brightened: number; darkened: number }>(
            "galleryStageBrightening",
            "stage-unselected",
            `stage-${selected}-selected`,
          );
          await scenario.evidence.record(`highlight-${selected}`, pixels);
          assert.ok(
            pixels.brightened > 64,
            `${selected} has no visible highlight`,
          );
          assert.ok(pixels.brightened > pixels.darkened * 4);
          assert.equal(
            (await guiApplication(g)).state.declarationIssue,
            undefined,
          );
          if (selected === "floor")
            await g.page.screenshot({
              path: resolve(
                scenario.evidence.directory,
                "floor-selection-page.png",
              ),
              fullPage: true,
            });
        }
        await openSettings(g, "scene");
        // One known, finite invalid authoring value after the recovery fix.
        await g.call("gallerySceneAction", "tuning", {
          method: "setLight",
          args: [-1],
        });
        const rejected = await waitApp(g, (v) =>
          Boolean(v.state.declarationIssue),
        );
        assert.equal(rejected.ready, true);
        assert.equal(rejected.error, undefined);
        await waitForGuiState(
          g,
          (s) =>
            s.texts.some(
              (t) =>
                t.symbol === "gui-settings-issue/text" &&
                t.text.includes("SCENE UPDATE REJECTED"),
            ),
          false,
        );
        const rejectedFrame = await g.call<{
          dataUrl: string;
          frame: { failedDrawCalls: number };
        }>("captureUnflushedViewer", "scene-rejection-visible", true);
        assert.equal(rejectedFrame.frame.failedDrawCalls, 0);
        await writeDataUrl(
          resolve(scenario.evidence.directory, "scene-rejection-visible.png"),
          rejectedFrame.dataUrl,
        );
        await scenario.evidence.record("rejected-frame", rejectedFrame.frame);
        await g.page.screenshot({
          path: resolve(
            scenario.evidence.directory,
            "scene-rejection-page.png",
          ),
          fullPage: true,
        });
        await scenario.evidence.record("rejected-observation", rejected);
        // This child World's real tree callback must work during primary rejection.
        await press(
          g,
          await find(g, "gui-scene-tree/row/floor", false),
          4 * (await spacing(g)),
          false,
        );
        await waitApp(g, (v) => v.state.tuning.focus === "floor");
        assert.ok(
          (await guiApplication(g)).state.declarationIssue,
          "invalid source lost its warning",
        );
        await selectSettingsPage(g, "projection", false);
        const projectionWarning = await g.call<{
          dataUrl: string;
          frame: { failedDrawCalls: number };
        }>("captureUnflushedViewer", "projection-rejection-visible", true);
        assert.equal(projectionWarning.frame.failedDrawCalls, 0);
        await writeDataUrl(
          resolve(
            scenario.evidence.directory,
            "projection-rejection-visible.png",
          ),
          projectionWarning.dataUrl,
        );
        await g.page.screenshot({
          path: resolve(
            scenario.evidence.directory,
            "projection-rejection-page.png",
          ),
          fullPage: true,
        });
        await press(
          g,
          await find(g, "gui-light/slider", false),
          4 * (await spacing(g)),
          false,
        );
        await g.page.keyboard.press("Home");
        await g.page.keyboard.press("ArrowUp");
        const corrected = await waitApp(
          g,
          (v) =>
            v.state.tuning.light >= 0 && v.state.declarationIssue === undefined,
        );
        assert.equal(corrected.ready, true);
        assert.equal(corrected.error, undefined);
        const frame = await g.capture("scene-corrected");
        assert.equal(frame.frame.failedDrawCalls, 0);
        const correctedInspection = await g.inspect();
        assert.deepEqual(
          ["gui-projector-floor", "gui-projector-base", "gui-demo"].map(
            (id) => sceneEntity(correctedInspection, id).id,
          ),
          identities,
        );
        await g.page.screenshot({
          path: resolve(
            scenario.evidence.directory,
            "scene-corrected-page.png",
          ),
          fullPage: true,
        });
        const alive = await g.call<{ before: bigint; after: bigint }>(
          "galleryIndependentWorld",
          "observe",
        );
        assert.ok(survivor.after > survivor.before);
        assert.ok(alive.after > alive.before);
        assert.ok(alive.after > survivor.after);
        await scenario.evidence.record("independent-world-progress", {
          survivor,
          alive,
        });
        assert.deepEqual(g.errors, []);
      } finally {
        await g.call("galleryIndependentWorld", "close");
      }
    },
  );
});

test("Gallery GUI retains presentation layer controls through animation and isolation", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "Scanner layer controls",
    environment("layers"),
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario, {
        initialPage: "gui",
        canvasShare: 1,
      });
      await enterWorkspace(g);
      await openSettings(g);
      const dial = await find(g, "gui-layer-step/dial");
      await press(g, dial, 0.5);
      await g.page.keyboard.press("End");
      await waitApp(g, (v) => Math.abs(v.state.layerStep - 1) < 1e-5);
      await press(g, await find(g, "gui-explode"), 0.5);
      await waitSpacing(g, 1);
      assert.deepEqual(
        (await find(g, "gui-layer-step/dial")).target,
        dial.target,
      );
      await press(g, await find(g, "gui-layer-step/dial"), 5);
      await g.page.keyboard.press("Home");
      await waitApp(g, (v) => Math.abs(v.state.layerStep - 0.15) < 1e-5);
      await waitSpacing(g, 0.15);
      for (const key of ["End", "Home", "End", "Home"]) {
        await g.page.keyboard.press(key);
        await waitApp(
          g,
          (v) =>
            Math.abs(v.state.layerStep - (key === "End" ? 1 : 0.15)) < 1e-5,
        );
        await waitSpacing(g, key === "End" ? 1 : 0.15);
      }
      await selectPresentationPage(g, "STYLE");
      await press(
        g,
        await find(g, "gui-reduced-motion"),
        5 * (await spacing(g)),
      );
      await waitApp(g, (v) => v.state.reducedMotion);
      await selectPresentationPage(g, "LAYERS");
      await press(g, await find(g, "gui-explode"), 5 * (await spacing(g)));
      await waitSpacing(g, 0.1);
      await press(g, await find(g, "gui-vector-only"), 0.5);
      await waitApp(g, (v) => v.state.vectorOnly);
      await g.waitFor(
        (i) =>
          !i.entities.some(
            (e) => e.metadata.symbolicId === "gui-projector-beam",
          ),
      );
      await press(g, await find(g, "gui-vector-only"), 0.5);
      await waitApp(g, (v) => !v.state.vectorOnly);
      await g.waitFor((i) =>
        i.entities.some((e) => e.metadata.symbolicId === "gui-projector-beam"),
      );
      await press(g, await find(g, "gui-explode"), 0.5);
      await waitSpacing(g, 0.15);
      const frame = await g.capture("scanner-exploded-settings");
      assert.equal(frame.frame.failedDrawCalls, 0);
      await g.page.screenshot({
        path: resolve(scenario.evidence.directory, "scanner-exploded-page.png"),
        fullPage: true,
      });
      assert.deepEqual(g.errors, []);
    },
  );
});

test("Gallery GUI retains presentation on all curved Surface modes with posed beam", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "Scanner curved presentation",
    environment("curved"),
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario, {
        initialPage: "gui",
        canvasShare: 1,
      });
      await enterWorkspace(g);
      await openSettings(g);
      await selectPresentationPage(g, "SURFACE");
      const modes = [
        ["flat", "outside"],
        ["cylinder", "outside"],
        ["cylinder", "inside"],
        ["sphere", "outside"],
        ["sphere", "inside"],
      ] as const;
      let beam: bigint | undefined;
      for (const [shape, facing] of modes) {
        if ((await guiApplication(g)).state.surfaceShape !== shape)
          await dropdown(g, "gui-surface-shape", shape.toUpperCase());
        if (
          shape !== "flat" &&
          (await guiApplication(g)).state.surfaceFacing !== facing
        )
          await dropdown(g, "gui-surface-facing", facing.toUpperCase());
        const current = await g.inspect();
        const entity = sceneEntity(current, "gui-projector-beam");
        beam ??= entity.id;
        assert.equal(entity.id, beam);
        const pose = entity.components.find(
          (c) => "weight" in c.fields && "source" in c.fields,
        )?.fields;
        assert.ok(pose, "beam has no authored MeshPose");
        assert.equal(pose.weight, shape === "flat" ? 0 : 1);
        assert.ok(
          String(pose.source).endsWith(
            `frustum-${shape === "flat" ? "cylinder" : shape}-${facing}.ippm`,
          ),
        );
        const curve = dynamicProperty(
          current,
          "gui-projector-beam",
          "curvature",
        );
        assert.equal(curve.kind, "vec2");
        const k = shape === "flat" ? 0 : (facing === "inside" ? -1 : 1) / 5.6;
        assert.ok(Math.abs((curve.value as readonly number[])[0]! - k) < 1e-6);
        assert.equal(
          (curve.value as readonly number[])[1],
          shape === "sphere" ? 1 : 0,
        );
        const captured = await g.capture(`scanner-${shape}-${facing}`);
        assert.equal(captured.frame.failedDrawCalls, 0);
        assert.ok(captured.summary.coverage > 0.08);
      }
      await selectPresentationPage(g, "LAYERS");
      await press(g, await find(g, "gui-explode"), 0.5);
      await waitSpacing(g, 0.9);
      await g.capture("scanner-curved-exploded");
      await g.page.screenshot({
        path: resolve(
          scenario.evidence.directory,
          "scanner-curved-exploded-page.png",
        ),
        fullPage: true,
      });
      // The raised presentation control remains reachable at its physical rank.
      await press(g, await find(g, "gui-layer-step/dial"), 4.5);
      await g.page.keyboard.press("Home");
      await waitSpacing(g, 0.15);
      assert.equal(
        Number(
          fieldsWith(await g.inspect(), "gui-demo", "curvature").curvature,
        ) < 0,
        true,
      );
      assert.deepEqual(g.errors, []);
    },
  );
});
