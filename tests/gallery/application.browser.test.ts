import assert from "node:assert/strict";
import { assertFrameRateData } from "./scenarios/frame-rate.js";
import test from "node:test";
import { runBrowserEnvironment } from "../harness/browser.js";
import {
  applicationEnvironment,
  openApplication,
} from "./drivers/browser-application.js";

test("shared application browser input, composed camera, DPR and cleanup", {
  // DPR rendering plus the real FPS observation interval runs on software GL
  // in CI; individual readiness and input operations retain bounded deadlines.
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "shared demo browser boundary",
    { ...applicationEnvironment, deviceScaleFactor: 2 },
    context.signal,
    async (environment) => {
      await environment.page.setViewportSize({ width: 960, height: 640 });
      const app = await openApplication(environment);
      const initial = await app.state();
      assert.equal(initial.options.scene, "shapes");
      assert.equal(initial.worlds.length, 2);
      const frame = await app.capture("initial-dpr");
      assert.equal(frame.frame.width, 1920);
      assert.equal(frame.frame.height, 1280);
      assert.ok(frame.summary.foregroundPixels > 1000);
      assert.ok(
        frame.scenePixels > frame.frame.width * frame.frame.height * 0.01,
        "the child scene paints below the toolbar",
      );
      assert.equal(
        await app.page.locator("button,select,input").count(),
        0,
        "application controls are native GUI",
      );
      await app.press("demo-display");
      await app.press("demo-performance-toggle");
      await app.page.mouse.click(900, 600);
      const historyDeadline = performance.now() + 15000;
      let history = (await app.state()).frameRate;
      while (
        (history?.rows.length ?? 0) < 3 &&
        performance.now() < historyDeadline
      ) {
        await app.page.waitForTimeout(100);
        history = (await app.state()).frameRate;
      }
      assertFrameRateData(history);
      await app.capture("fps-history");
      await app.action("performance", false);
      assert.equal((await app.state()).frameRate, undefined);
      // A physical drag admitted by the toolbar must not orbit the scene behind it.
      const control = await app.coordinates("demo-scene-picker");
      await app.drag([control.x, control.y], [control.x + 30, control.y]);
      const shielded = await app.state();
      assert.deepEqual(shielded.camera, initial.camera);
      await app.page.mouse.click(900, 600);
      await app.press("demo-scene-picker");
      await app.press("demo-scene-picker/options/lighting");
      const lighting = await app.waitForOption("scene", "lighting");
      assert.equal(lighting.options.scene, "lighting");
      assert.deepEqual(
        lighting.cameraPath[0]!.parent,
        initial.cameraPath[0]!.parent,
      );
      assert.notDeepEqual(lighting.sceneWorlds, initial.sceneWorlds);
      assert.equal(lighting.worlds.length, 2, "the replaced scene is retired");
      await app.action("pause");
      const before = await app.state();
      await app.drag([650, 400], [720, 440]);
      const rotated = await app.state();
      assert.notDeepEqual(
        rotated.camera,
        before.camera,
        "unhandled canvas drag navigates the composed camera",
      );
      await app.page.mouse.move(700, 420);
      await app.page.mouse.wheel(0, 120);
      await app.settle();
      const zoomed = await app.state();
      assert.notDeepEqual(
        zoomed.camera,
        rotated.camera,
        "unhandled wheel navigates the composed camera",
      );
      await app.capture("camera-navigated");
      await app.press("demo-display");
      await app.press("demo-skin-picker");
      await app.press("demo-skin-picker/options/cybertron");
      await app.waitForOption("skin", "cybertron");
      const logicalBefore = await app.call<{ bounds: unknown }>(
        "point",
        "demo-scene-picker",
      );
      await app.press("demo-render-scale");
      await app.press("demo-render-scale/options/0.5");
      await app.waitForOption("renderScale", 0.5);
      const scaled = await app.capture("scaled-cybertron");
      assert.equal(scaled.frame.width, 960);
      assert.equal(scaled.frame.height, 640);
      assert.deepEqual(
        (await app.call<{ bounds: unknown }>("point", "demo-scene-picker"))
          .bounds,
        logicalBefore.bounds,
        "DPR and render scale preserve logical GUI bounds",
      );
      await app.page.mouse.click(900, 600);
      await app.press("demo-scene-picker");
      await app.press("demo-scene-picker/options/shapes");
      assert.equal(
        (await app.waitForOption("scene", "shapes")).worlds.length,
        2,
      );
      assert.deepEqual(app.errors, []);
      await app.call("observeCleanup");
      await app.page.evaluate(() =>
        window.dispatchEvent(new Event("pagehide")),
      );
      let cleanup: { worlds: bigint[]; error?: string } | undefined;
      const deadline = performance.now() + 10_000;
      while (!cleanup && performance.now() < deadline) {
        cleanup = await app.call("cleanupState");
        if (!cleanup) await new Promise((resolve) => setTimeout(resolve, 25));
      }
      assert.ok(cleanup, "beforeClose completed application cleanup");
      assert.equal(cleanup.error, undefined);
      assert.deepEqual(
        cleanup.worlds,
        [initial.cameraPath[0]!.parent.id],
        "beforeClose retires owned scene Worlds before shutting down the Host",
      );
      await app.page.waitForFunction(
        () => !document.querySelector("#ipp-demo-canvas"),
      );
    },
  );
});

test("cockpit carousel browser swipe, wheel priority and cancellation", {
  // The complete sequence includes several rendered transitions and captures.
  // Software WebGL needs a larger suite budget; individual input/readiness
  // assertions retain their short deadlines so a stuck gesture still fails.
  timeout: 600_000,
}, async (context) => {
  await runBrowserEnvironment(
    "cockpit browser drag",
    applicationEnvironment,
    context.signal,
    async (environment) => {
      await environment.page.setViewportSize({ width: 1280, height: 900 });
      const app = await openApplication(environment);
      await app.navigate("cockpit");
      await app.waitForOption("scene", "cockpit");
      await app.call("options", {
        carousel: 1,
        lightsPlaying: false,
        atmospherePlaying: false,
        scanning: false,
      });
      const stopped = async () =>
        app.waitFor(
          (state) =>
            state.controllers?.every((controller) => !controller.transition) ??
            true,
        );
      await stopped();
      const initial = await app.state();
      await environment.evidence.record("cockpit-input-progress", {
        phase: "ready",
        carousel: initial.options.carousel,
      });
      const captureLog = await app.page.evaluateHandle(() => {
        const element =
          document.querySelector<HTMLCanvasElement>("#ipp-demo-canvas")!;
        const samples: boolean[] = [];
        element.addEventListener("pointermove", (event) => {
          if (event.buttons === 1)
            samples.push(element.hasPointerCapture(event.pointerId));
        });
        return samples;
      });
      const waitForRelease = async (changed = false) => {
        // Reuse the driver input deadline: software rendering can spend most
        // of it admitting the press before its queued release reaches the app.
        const deadline = performance.now() + 30_000;
        for (;;) {
          const snapshot = await app.state();
          if (
            snapshot.options.carouselDragging === false &&
            (!changed || Number(snapshot.options.carousel) > 1)
          )
            return snapshot;
          if (performance.now() >= deadline) {
            await environment.evidence.record("cockpit-release-diagnostics", {
              changed,
              snapshot,
              ...(await app.call<Record<string, unknown>>(
                "cockpitInputDiagnostics",
              )),
            });
            assert.fail(
              "The admitted gesture finishes and clears its drag state",
            );
          }
        }
      };
      const swipe = async () => {
        const from = await app.cockpitCoordinates(1, "cockpit-header-1");
        // No inspection or frame wait between down, move and up: the DOM adapter
        // must retain this burst while native capture admission is asynchronous.
        await app.page.mouse.move(from.x, from.y);
        await app.page.mouse.down();
        await app.page.mouse.move(from.x - 256, from.y, { steps: 2 });
        await app.page.mouse.up();
        return waitForRelease(true);
      };
      try {
        const swiped = await swipe();
        assert.equal(
          Number(swiped.options.carousel) % 1,
          0,
          "Release snaps to a complete pane",
        );
        assert.deepEqual(
          swiped.camera,
          initial.camera,
          "The shared rotor moves; the cockpit Camera stays fixed",
        );
        const capturedMoves = await captureLog.jsonValue();
        assert.ok(
          capturedMoves.length >= 2,
          "The quick swipe delivered real DOM pointer moves",
        );
        assert.ok(
          capturedMoves.every(Boolean),
          "DOM pointer capture retains the quick swipe",
        );
        await environment.evidence.record("cockpit-input-progress", {
          phase: "quick-swipe",
          carousel: swiped.options.carousel,
          capturedMoves,
        });
        await stopped();
        await app.capture("cockpit-browser-swiped");
        await app.call("options", { carousel: 0 });
        await stopped();
        const beforeWidget = await app.state();
        const slider = await app.cockpitCoordinates(
          0,
          "cockpit-gain/slider",
          0.25,
        );
        await app.drag([slider.x, slider.y], [slider.x + 100, slider.y]);
        const widgetDeadline = performance.now() + 15_000;
        let afterWidget = await app.state();
        while (
          afterWidget.options.gain === beforeWidget.options.gain ||
          afterWidget.options.carouselDragging !== false
        ) {
          assert.ok(
            performance.now() < widgetDeadline,
            "The projected slider commits its value",
          );
          afterWidget = await app.state();
        }
        assert.equal(
          afterWidget.options.carousel,
          beforeWidget.options.carousel,
          "A native slider owns its pointer without rotating the carousel",
        );
        assert.notEqual(
          afterWidget.options.gain,
          beforeWidget.options.gain,
          "The projected slider receives the drag",
        );
        await environment.evidence.record("cockpit-input-progress", {
          phase: "widget-drag",
          carousel: afterWidget.options.carousel,
          gain: afterWidget.options.gain,
        });

        const waitForWheelFocus = async (expected: number) => {
          const deadline = performance.now() + 15_000;
          for (;;) {
            const state = await app.state();
            if (Math.abs(Number(state.options.carousel) - expected) < 0.0001)
              return state;
            assert.ok(
              performance.now() < deadline,
              `Wheel selects carousel ${expected}`,
            );
          }
        };
        await app.call("options", { carousel: 1 });
        await stopped();
        let wheelHeader = await app.cockpitCoordinates(1, "cockpit-header-1");
        await app.page.mouse.move(wheelHeader.x, wheelHeader.y);
        await app.page.mouse.wheel(100, 0);
        await waitForWheelFocus(1.25);
        await stopped();
        wheelHeader = await app.cockpitCoordinates(1, "cockpit-header-1");
        await app.page.mouse.move(wheelHeader.x, wheelHeader.y);
        await app.page.mouse.wheel(0, 100);
        await waitForWheelFocus(1.5);
        await stopped();
        wheelHeader = await app.cockpitCoordinates(1, "cockpit-header-1");
        await app.page.mouse.move(wheelHeader.x, wheelHeader.y);
        // Dispatch without native outcome waits: every fractional delta must survive.
        await Promise.all(
          Array.from({ length: 10 }, () => app.page.mouse.wheel(10, 0)),
        );
        await waitForWheelFocus(1.75);
        await stopped();
        await app.call("options", { carousel: 0, gain: 1 });
        await stopped();
        const wheelValue = await app.cockpitCoordinates(
          0,
          "cockpit-gain/slider",
        );
        await app.page.mouse.move(wheelValue.x, wheelValue.y);
        await app.page.mouse.wheel(0, -100);
        await app.settle();
        assert.equal(
          (await app.state()).options.carousel,
          0,
          "A hovered slider at its bound still owns the wheel",
        );
        // The fixed bridge framing puts a side pane under the old upper-right
        // point. The lower-right console corner stays outside every GUI Surface.
        const viewport = app.page.viewportSize()!;
        await app.page.mouse.move(viewport.width - 8, viewport.height - 8);
        await app.page.mouse.wheel(100, 0);
        await app.settle();
        assert.equal(
          (await app.state()).options.carousel,
          0,
          "Wheel over the background never moves the windows",
        );
        await app.call("options", { carousel: 2 });
        await stopped();
        const wheelBody = await app.cockpitCoordinates(
          2,
          "cockpit-content-2",
          0.96,
        );
        await app.page.mouse.move(wheelBody.x, wheelBody.y);
        await app.page.mouse.wheel(0, -100);
        await waitForWheelFocus(1.75);
        await stopped();
        const wheelState = await app.state();
        assert.deepEqual(
          wheelState.camera,
          initial.camera,
          "Wheel moves the shared rotor while the Camera stays fixed",
        );
        await app.capture("cockpit-browser-wheel");
        await environment.evidence.record("cockpit-input-progress", {
          phase: "wheel",
          carousel: wheelState.options.carousel,
        });
        await app.call("startCockpitInputTrace");
        await app.call("options", { carousel: 1 });
        await stopped();

        for (const kind of ["pointercancel", "blur"] as const) {
          const header = await app.cockpitCoordinates(1, "cockpit-header-1");
          await app.page.mouse.move(header.x, header.y);
          await app.page.mouse.down();
          try {
            await app.waitForOption("carouselDragging", true);
          } catch (error) {
            await environment.evidence.record("cockpit-down-diagnostics", {
              kind,
              header,
              ...(await app.call<
                Awaited<
                  ReturnType<
                    typeof import("./pages/application.js").cockpitInputDiagnostics
                  >
                >
              >("cockpitInputDiagnostics")),
            });
            throw error;
          }
          await app.page.evaluate((kind) => {
            if (kind === "blur") window.dispatchEvent(new Event("blur"));
            else
              document.querySelector("#ipp-demo-canvas")!.dispatchEvent(
                new PointerEvent("pointercancel", {
                  pointerId: 1,
                  pointerType: "mouse",
                  isPrimary: true,
                  bubbles: true,
                }),
              );
          }, kind);
          await waitForRelease();
          await app.page.mouse.up();
          assert.equal(
            await app.page
              .locator("#ipp-demo-canvas")
              .evaluate((element) => element.hasPointerCapture(1)),
            false,
            `${kind} releases DOM capture`,
          );
          await environment.evidence.record("cockpit-input-progress", {
            phase: kind,
          });
        }
        await app.call("options", { carousel: 1 });
        await stopped();
        await swipe();
        await environment.evidence.record("cockpit-input-progress", {
          phase: "reused-pointer",
        });
        await stopped();
        await app.capture("cockpit-browser-reused-pointer");
        assert.deepEqual(app.errors, []);
      } finally {
        await captureLog.dispose();
      }
    },
  );
});
