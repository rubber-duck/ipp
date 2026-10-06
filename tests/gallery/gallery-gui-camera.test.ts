import type {
  RenderStatisticsSnapshot,
  SurfaceCacheRecord,
} from "@ipp/client/diagnostics";
import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import type { Inspection } from "@ipp/client";
import { runBrowserEnvironment } from "../harness/browser.js";
import {
  galleryEnvironment,
  openGallery,
  transform,
} from "./drivers/browser-gallery.js";
import {
  control,
  projectContent,
  type ProjectedPoint,
} from "./gallery-gui-oracle.js";
import type { GalleryGuiState } from "./pages/viewer.js";
import {
  dropdown,
  enterWorkspace,
  find,
  openSettings,
  press,
  selectPresentationPage,
  spacing,
  waitApp,
  waitSpacing,
} from "./support/gallery-scanner.js";
import { guiApplication } from "./support/gallery-gui.js";

const environment = {
  ...galleryEnvironment,
  evidenceParent: resolve("target/integration-artifacts/gallery-gui-camera"),
};

/** Gallery panel cache policy, restated independently of scene.tsx. */
const CACHE_DIRECT_DISTANCE = 20;
/** World distance from the camera to the panel anchor, from public inspection. */
function panelDistance(inspection: Inspection): number {
  const camera = transform(inspection);
  const panel = transform(inspection, "gui-demo");
  return Math.hypot(
    ...["x", "y", "z"].map(
      (axis) => Number(panel[axis]) - Number(camera[axis]),
    ),
  );
}

function cameraChanged(before: Record<string, unknown>, after: Inspection) {
  const current = transform(after);
  return ["x", "y", "z", "qx", "qy", "qz", "qw"].some(
    (field) => Math.abs(Number(current[field]) - Number(before[field])) > 1e-5,
  );
}

test("Scanner GUI owns panel gestures and admits background camera gestures", {
  timeout: 120_000,
}, async (context) => {
  await runBrowserEnvironment(
    "GUI camera input routing",
    environment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario, { initialPage: "gui" });
      await enterWorkspace(g);
      await press(g, await find(g, "gui-scan"), 2 * (await spacing(g)));
      await waitApp(g, (app) => !app.state.autoscan);
      const chooseShape = async (shape: "FLAT" | "CYLINDER") => {
        await openSettings(g);
        // Reduced motion and the paused scanner provide stable cached pixels
        // for exact camera-only cache work counts.
        if (
          shape === "CYLINDER" ||
          (await guiApplication(g)).state.reducedMotion
        ) {
          await selectPresentationPage(g, "STYLE");
          await press(
            g,
            await find(g, "gui-reduced-motion"),
            5 * (await spacing(g)),
          );
          await waitApp(
            g,
            (app) => app.state.reducedMotion === (shape === "CYLINDER"),
          );
        }
        await selectPresentationPage(g, "SURFACE");
        await dropdown(g, "gui-surface-shape", shape);
        await waitApp(
          g,
          (app) => app.state.surfaceShape === shape.toLowerCase(),
        );
        await press(
          g,
          await find(g, "gui-settings-close"),
          4 * (await spacing(g)),
        );
        await waitApp(g, (app) => !app.state.app.settings);
        await g.page.keyboard.press("Escape");
        await g.page.mouse.move(0, 0);
        await g.capture(`camera-${shape.toLowerCase()}-ready`);
      };
      // Curved presentation caches the coincident workspace as one image.
      await chooseShape("CYLINDER");
      await g.page.locator("#ipp-world-canvas").scrollIntoViewIfNeeded();
      const canvas = await g.page.locator("#ipp-world-canvas").boundingBox();
      assert.ok(canvas);
      // Pointer distances as a share of the canvas width, so gestures keep
      // their extent in the scene at any canvas size.
      const span = (share: number) => canvas.width * share;

      const point = async (
        symbol: string,
        fractionX = 0.5,
        fractionY = 0.5,
      ) => {
        const target = await find(g, symbol);
        const [x, y, width, height] = target.bounds;
        const rank =
          symbol === "gui-pulse"
            ? 3
            : symbol === "gui-gain/dial" || symbol === "gui-scan"
              ? 2
              : symbol === "gui-password" ||
                  symbol === "gui-callsign" ||
                  symbol === "gui-log-open" ||
                  symbol === "gui-settings-open"
                ? 1
                : 0;
        const [projected] = await projectContent(
          g,
          [[x + width * fractionX, y + height * fractionY]],
          rank * (await spacing(g)),
        );
        assert.ok(projected);
        return projected;
      };
      const panelEdge = async () => {
        const corners = await projectContent(
          g,
          [
            [0, 0],
            [1036, 0],
            [0, 672],
            [1036, 672],
          ],
          3 * (await spacing(g)),
        );
        const bounds = await g.page.locator("#ipp-world-canvas").boundingBox();
        assert.ok(bounds);
        const center = corners.reduce<[number, number]>(
          (sum, corner) => [
            sum[0] + corner.clientX / corners.length,
            sum[1] + corner.clientY / corners.length,
          ],
          [0, 0],
        );
        const topLeft = corners[0]!;
        const topRight = corners[1]!;
        const bottomLeft = corners[2]!;
        const bottomRight = corners[3]!;
        const edges: Array<readonly [ProjectedPoint, ProjectedPoint]> = [
          [topLeft, topRight],
          [topRight, bottomRight],
          [bottomRight, bottomLeft],
          [bottomLeft, topLeft],
        ];
        const candidates = edges.map(([a, b]) => {
          const midpoint = [
            (a.clientX + b.clientX) / 2,
            (a.clientY + b.clientY) / 2,
          ] as const;
          const length = Math.hypot(
            midpoint[0] - center[0],
            midpoint[1] - center[1],
          );
          const outward = [
            (midpoint[0] - center[0]) / length,
            (midpoint[1] - center[1]) / length,
          ] as const;
          const tangentLength = Math.hypot(
            b.clientX - a.clientX,
            b.clientY - a.clientY,
          );
          const tangent = [
            (b.clientX - a.clientX) / tangentLength,
            (b.clientY - a.clientY) / tangentLength,
          ] as const;
          const roomX =
            Math.abs(outward[0]) < 1e-6
              ? Number.POSITIVE_INFINITY
              : (outward[0] > 0
                  ? bounds.x + bounds.width - midpoint[0]
                  : midpoint[0] - bounds.x) / Math.abs(outward[0]);
          const roomY =
            Math.abs(outward[1]) < 1e-6
              ? Number.POSITIVE_INFINITY
              : (outward[1] > 0
                  ? bounds.y + bounds.height - midpoint[1]
                  : midpoint[1] - bounds.y) / Math.abs(outward[1]);
          const room = Math.min(roomX, roomY);
          return { midpoint, outward, tangent, room };
        });
        candidates.sort((a, b) => b.room - a.room);
        const edge = candidates[0]!;
        assert.ok(
          edge.room > span(0.035),
          "GUI demo leaves no camera drag area",
        );
        const margin = span(0.016);
        return {
          outside: [
            edge.midpoint[0] + edge.outward[0] * margin,
            edge.midpoint[1] + edge.outward[1] * margin,
          ] as [number, number],
          inside: [
            edge.midpoint[0] - edge.outward[0] * margin,
            edge.midpoint[1] - edge.outward[1] * margin,
          ] as [number, number],
          tangent: edge.tangent,
        };
      };

      const unchangedAfterDrag = async (
        from: readonly [number, number],
        to: readonly [number, number],
      ) => {
        const before = transform(await g.inspect());
        await g.drag(from, to);
        const after = transform(await g.settle());
        assert.deepEqual(after, before);
      };

      let edge = await panelEdge();

      // Camera motion changes projected resolution. Unchanged frames reuse
      // the resulting image without repainting or uploading.
      const cacheState = async (label: string) => {
        const panel = (await g.inspect()).entities.find(
          ({ metadata }) => metadata.symbolicId === "gui-demo",
        )!.id;
        const { frame } = await g.capture(label);
        const records = frame.statistics!.surfaces!.surfaceCaches as
          | readonly SurfaceCacheRecord[]
          | undefined;
        assert.ok(records, "Surface cache diagnostics are unavailable");
        const { ingress: _ingress, ...stats } = frame.statistics!;
        await scenario.evidence.record(`${label}-surface-cache`, stats);
        return {
          record: records.find(({ entity }) => entity === panel),
          statistics: frame.statistics,
          repaints: Number(
            frame.statistics!.surfaces!.totalSurfaceCacheRepaints,
          ),
          allocations: Number(
            frame.statistics!.surfaces!.totalSurfaceCacheAllocations,
          ),
          uploaded: Number(frame.statistics!.frame.totalUploadedBytes),
        };
      };
      const cacheUntil = async (
        label: string,
        ready: (record?: SurfaceCacheRecord) => boolean,
      ) => {
        const deadline = performance.now() + 10_000;
        for (;;) {
          const state = await cacheState(label);
          if (ready(state.record)) return state;
          assert.ok(
            performance.now() < deadline,
            `${label}: cache record stayed ${state.record?.mode}`,
          );
        }
      };
      const reusedBand = (band: number) => (record?: SurfaceCacheRecord) =>
        record?.mode === "reused" && record.band === band;
      const dollyOut = async (minimum: number) => {
        for (let step = 0; step < 12; step++) {
          const distance = panelDistance(await g.inspect());
          if (distance >= minimum) return distance;
          const before = transform(await g.inspect());
          await g.page.mouse.move(...edge.outside);
          await g.page.mouse.wheel(0, 120);
          await g.waitFor((inspection) => cameraChanged(before, inspection));
        }
        throw new Error(`Camera did not dolly beyond ${minimum} m`);
      };
      await new Promise((resolve) => setTimeout(resolve, 550));
      assert.ok(panelDistance(await g.inspect()) < CACHE_DIRECT_DISTANCE);
      const near = await cacheUntil("camera-cache-near", reusedBand(0));
      const layerImages =
        near.record!.residentBytes /
        (4 * near.record!.capacityWidth * near.record!.capacityHeight);
      assert.equal(
        layerImages,
        1,
        "coincident workspace layers should share one cache image",
      );
      const bandOneDistance = await dollyOut(CACHE_DIRECT_DISTANCE * 1.2);
      assert.ok(bandOneDistance < 2 * CACHE_DIRECT_DISTANCE);
      const bandOne = await cacheUntil("camera-cache-band1", reusedBand(1));
      assert.ok(bandOne.repaints - near.repaints >= layerImages);
      edge = await panelEdge();
      const beforeOrbit = transform(await g.inspect());
      await g.drag(edge.outside, [
        edge.outside[0] + edge.tangent[0] * span(0.07),
        edge.outside[1] + edge.tangent[1] * span(0.07),
      ]);
      await g.waitFor((inspection) => cameraChanged(beforeOrbit, inspection));
      const orbited = await cacheState("camera-cache-orbited");
      assert.ok(cameraChanged(beforeOrbit, await g.inspect()));
      assert.equal(orbited.record?.mode, "reused");
      assert.equal(orbited.record?.band, 1);
      // Resolution follows projected pixels, including camera orientation.
      // A second unchanged frame must reuse that allocation without uploads.
      const orbitReuse = await cacheUntil(
        "camera-cache-orbit-reuse",
        reusedBand(1),
      );
      assert.equal(orbitReuse.repaints, orbited.repaints);
      assert.equal(orbitReuse.allocations, orbited.allocations);
      assert.equal(orbitReuse.uploaded, orbited.uploaded);
      assert.equal(orbitReuse.statistics!.frame.uploadedBytes, 0);
      edge = await panelEdge();
      await dollyOut(2 * CACHE_DIRECT_DISTANCE * 1.2);
      const bandTwo = await cacheUntil("camera-cache-band2", reusedBand(2));
      assert.ok(bandTwo.repaints - orbited.repaints >= layerImages);
      assert.ok(bandTwo.record!.width < near.record!.width);
      assert.ok(bandTwo.record!.height < near.record!.height);
      for (const state of [near, bandOne, orbited, bandTwo]) {
        assert.equal(
          Math.max(state.record!.width, state.record!.height) % 8,
          0,
        );
        assert.ok(state.record!.width <= state.record!.capacityWidth);
        assert.ok(state.record!.height <= state.record!.capacityHeight);
        assert.equal(
          state.record!.residentBytes,
          4 *
            layerImages *
            state.record!.capacityWidth *
            state.record!.capacityHeight,
        );
      }
      assert.equal(
        bandTwo.statistics!.surfaces!.surfaceCacheResidentBytes,
        bandTwo.record!.residentBytes,
      );
      await g.page.locator("#reset-camera").click();
      await cacheUntil("camera-cache-reset", reusedBand(0));
      await chooseShape("FLAT");
      // Decorative header content owns a press even though it is no control.
      const [header] = await projectContent(g, [[220, 46]]);
      assert.ok(header);
      edge = await panelEdge();
      await unchangedAfterDrag([header.clientX, header.clientY], edge.outside);

      const gainSelector = {
        role: "slider",
        symbol: "gui-gain/dial",
      } as const;
      const dial = await find(g, gainSelector.symbol);
      const [dialX, dialY, dialWidth, dialHeight] = dial.bounds;
      // A dial spans its range over 2.5 sides of vertical pointer travel.
      const dialTravel = 2.5 * Math.min(dialWidth, dialHeight);
      const sliderStart = await point(gainSelector.symbol);
      const [sliderEnd] = await projectContent(
        g,
        [[dialX + dialWidth / 2, dialY + dialHeight / 2 - 0.65 * dialTravel]],
        2 * (await spacing(g)),
      );
      assert.ok(sliderEnd);
      await unchangedAfterDrag(
        [sliderStart.clientX, sliderStart.clientY],
        [sliderEnd.clientX, sliderEnd.clientY],
      );

      // Keep a real browser pointer held while replaying a high-rate move
      // stream. Six Playwright drag steps did not expose the request backlog
      // caused by updating hundreds of waveform decoration nodes per gain edit.
      await g.call("galleryGuiAction", gainSelector, {
        kind: "scalar",
        value: 0.1,
      });
      const beforeStream = transform(await g.inspect());
      await g.capture("sustained-slider-before");
      const dialCorners = await projectContent(
        g,
        [
          [dialX, dialY],
          [dialX + dialWidth, dialY + dialHeight],
        ],
        2 * (await spacing(g)),
      );
      const sliderRegion = [
        Math.min(...dialCorners.map((p) => p.x)),
        Math.min(...dialCorners.map((p) => p.y)),
        Math.max(...dialCorners.map((p) => p.x)),
        Math.max(...dialCorners.map((p) => p.y)),
      ];
      // Accumulated render work since the worker started, read at a frame.
      const renderTotals = async (label: string) => {
        const { frame } = await g.call<{
          frame: {
            tick: bigint;
            statistics: RenderStatisticsSnapshot | undefined;
          };
        }>("captureUnflushedViewer", label);
        return {
          tick: Number(frame.tick),
          rebuilds: Number(frame.statistics!.gui!.totalGuiRebuilds),
          uploaded: Number(frame.statistics!.frame.totalUploadedBytes),
        };
      };
      // Retained analytic text leaves the idle demo only small per-frame
      // uploads from its other animated content; measure them as the baseline.
      const idleStart = await renderTotals("sustained-slider-idle-start");
      let idleEnd = idleStart;
      while (idleEnd.tick - idleStart.tick < 20)
        idleEnd = await renderTotals("sustained-slider-idle-end");
      await g.call("observeGalleryGuiInput");
      await g.page.mouse.move(sliderStart.clientX, sliderStart.clientY);
      await g.page.mouse.down();
      let dragStart = idleEnd;
      let dragEnd = idleEnd;
      try {
        dragStart = await renderTotals("sustained-slider-drag-start");
        await g.page.evaluate(
          async ({ from, to }) => {
            const canvas =
              document.querySelector<HTMLCanvasElement>("#ipp-world-canvas")!;
            await new Promise<void>((resolve) => {
              let count = 0;
              const timer = setInterval(() => {
                const fraction = (count % 120) / 119;
                canvas.dispatchEvent(
                  new PointerEvent("pointermove", {
                    bubbles: true,
                    pointerId: 1,
                    pointerType: "mouse",
                    isPrimary: true,
                    buttons: 1,
                    clientX:
                      from.clientX + (to.clientX - from.clientX) * fraction,
                    clientY:
                      from.clientY + (to.clientY - from.clientY) * fraction,
                  }),
                );
                if (++count === 360) {
                  clearInterval(timer);
                  resolve();
                }
              }, 8);
            });
          },
          { from: sliderStart, to: sliderEnd },
        );
        await g.page.mouse.move(sliderEnd.clientX, sliderEnd.clientY);
        dragEnd = await renderTotals("sustained-slider-drag-end");
      } finally {
        await g.page.mouse.up();
      }
      const inputStream = await g.call<{
        sent: number;
        completed: number;
        peakPending: number;
        errors: string[];
      }>("finishGalleryGuiInputObservation");
      await scenario.evidence.record("sustained-slider-input", inputStream);

      // The dial's fill and pointer are retained parameterized shapes;
      // the readout updates its glyph batch. Keep the former rail scenario's
      // bounds on per-frame rebuilds and bytes per rebuild for this stream.
      const sliderBoxes = 3;
      const maxBoxUploadBytes = 3072;
      const idleUploadPerTick =
        (idleEnd.uploaded - idleStart.uploaded) /
        (idleEnd.tick - idleStart.tick);
      const dragTicks = dragEnd.tick - dragStart.tick;
      const dragRebuilds = dragEnd.rebuilds - dragStart.rebuilds;
      const dragExtraUploads =
        dragEnd.uploaded - dragStart.uploaded - idleUploadPerTick * dragTicks;
      await scenario.evidence.record("sustained-slider-render-work", {
        idleTicks: idleEnd.tick - idleStart.tick,
        idleUploadPerTick,
        dragTicks,
        dragRebuilds,
        dragExtraUploads,
      });
      assert.ok(dragTicks > 10, `drag spanned only ${dragTicks} frames`);
      assert.ok(dragRebuilds > 0, "the dragged slider never repainted");
      assert.ok(
        dragRebuilds <= sliderBoxes * dragTicks,
        `${dragRebuilds} GUI rebuilds in ${dragTicks} drag frames`,
      );
      assert.ok(
        dragExtraUploads <= maxBoxUploadBytes * dragRebuilds,
        `${dragExtraUploads} bytes beyond idle for ${dragRebuilds} rebuilt boxes`,
      );
      assert.ok(inputStream.sent >= 362);
      assert.ok(
        inputStream.peakPending <= 64,
        `sustained input retained ${inputStream.peakPending} pending requests`,
      );
      assert.deepEqual(inputStream.errors, []);
      assert.equal(inputStream.completed, inputStream.sent);
      assert.deepEqual(transform(await g.settle()), beforeStream);
      await waitApp(g, (app) => Math.abs(app.state.gain - 0.75) < 1e-6);
      const streamed = await g.call<GalleryGuiState>("galleryGuiState");
      const gain = control(streamed, gainSelector).value;
      assert.equal(gain.kind, "scalar");
      assert.ok(Math.abs(gain.value - 0.75) < 1e-6);
      assert.equal(
        await g.page.locator("#status").getAttribute("data-state"),
        "ready",
      );
      const streamedFrame = await g.capture("sustained-slider-complete");
      assert.ok(streamedFrame.summary.coverage > 0.08);
      const sliderPixels = await g.call<{ changedPixels: number }>(
        "compareViewerCaptureRegion",
        "sustained-slider-before",
        "sustained-slider-complete",
        sliderRegion,
      );
      assert.ok(
        sliderPixels.changedPixels > 80,
        "streamed slider value did not reach the rendered thumb",
      );

      await g.call("observeGalleryGuiInput");
      const beforeQuickDrag = transform(await g.inspect());
      await g.drag(edge.outside, [
        edge.outside[0] + edge.tangent[0] * span(0.04),
        edge.outside[1] + edge.tangent[1] * span(0.04),
      ]);
      await scenario.evidence.record("quick-background-drag", {
        input: await g.call("finishGalleryGuiInputObservation"),
        edge,
        currentEdge: await panelEdge(),
        canvas: await g.page.locator("canvas").first().boundingBox(),
        before: beforeQuickDrag,
        after: transform(await g.inspect()),
      });
      await g.waitFor((inspection) =>
        cameraChanged(beforeQuickDrag, inspection),
      );

      edge = await panelEdge();
      const beforeCrossing = transform(await g.inspect());
      await g.drag(edge.outside, edge.inside);
      await g.waitFor((inspection) =>
        cameraChanged(beforeCrossing, inspection),
      );

      edge = await panelEdge();
      // No ScrollView consumes a wheel over the LOG button, so the
      // runtime reports it unhandled and the camera zooms over the panel.
      const pulse = await point("gui-log-open");
      const beforePanelWheel = transform(await g.inspect());
      await g.page.mouse.move(pulse.clientX, pulse.clientY);
      await g.page.mouse.wheel(0, 120);
      await g.waitFor((inspection) =>
        cameraChanged(beforePanelWheel, inspection),
      );
      edge = await panelEdge();

      const beforeOutsideWheel = transform(await g.inspect());
      await g.page.mouse.move(...edge.outside);
      await g.page.mouse.wheel(0, 120);
      await g.waitFor((inspection) =>
        cameraChanged(beforeOutsideWheel, inspection),
      );

      const first = await point("gui-log-open");
      const second = await point("gui-settings-open");
      // The semantic interaction state names the one hovered control; the
      // skin paints that state, so a stale hover would also keep its look.
      const hovered = async (symbol: string) =>
        control(await g.call<GalleryGuiState>("galleryGuiState"), {
          role: "button",
          symbol,
        }).interaction.hovered;
      const burstStarted = performance.now();
      await g.page.locator("#ipp-world-canvas").evaluate(
        (canvas, points) => {
          for (let index = 0; index < 40; index++) {
            const point = points[index % 2]!;
            canvas.dispatchEvent(
              new PointerEvent("pointermove", {
                bubbles: true,
                pointerId: 91,
                pointerType: "mouse",
                isPrimary: true,
                clientX: point[0]!,
                clientY: point[1]!,
              }),
            );
          }
          const point = points[1]!;
          canvas.dispatchEvent(
            new PointerEvent("pointermove", {
              bubbles: true,
              pointerId: 91,
              pointerType: "mouse",
              isPrimary: true,
              clientX: point[0]!,
              clientY: point[1]!,
            }),
          );
        },
        [
          [first.clientX, first.clientY],
          [second.clientX, second.clientY],
        ],
      );
      for (;;) {
        if (await hovered("gui-settings-open")) break;
        assert.ok(
          performance.now() - burstStarted < 550,
          "rapid hover burst retained a stale control",
        );
        await new Promise((resolve) => setTimeout(resolve, 16));
      }
      assert.equal(
        await hovered("gui-log-open"),
        false,
        "rapid hover burst left the earlier control hovered",
      );

      // The browser adapter prevents the context menu over the canvas, so
      // a secondary press can be the GUI's context request. The gallery
      // camera has no secondary-button gesture: a right drag over the
      // background moves nothing and leaves no gesture behind, so the next
      // primary drag still orbits.
      edge = await panelEdge();
      const contextMenu = await g.page
        .locator("#ipp-world-canvas")
        .evaluate((canvas, [x, y]) => {
          const event = new MouseEvent("contextmenu", {
            bubbles: true,
            cancelable: true,
            clientX: x,
            clientY: y,
            button: 2,
          });
          return canvas.dispatchEvent(event) ? "shown" : "prevented";
        }, edge.outside);
      assert.equal(contextMenu, "prevented");
      const beforeRightDrag = transform(await g.inspect());
      await g.page.mouse.move(...edge.outside);
      await g.page.mouse.down({ button: "right" });
      await g.page.mouse.move(
        edge.outside[0] + edge.tangent[0] * span(0.05),
        edge.outside[1] + edge.tangent[1] * span(0.05),
        { steps: 6 },
      );
      await g.page.mouse.up({ button: "right" });
      assert.deepEqual(
        transform(await g.settle()),
        beforeRightDrag,
        "a secondary-button drag moved the gallery camera",
      );
      const beforeOrbitAfterRight = transform(await g.inspect());
      await g.page.mouse.move(...edge.outside);
      await g.page.mouse.down();
      await g.page.mouse.move(
        edge.outside[0] + edge.tangent[0] * span(0.05),
        edge.outside[1] + edge.tangent[1] * span(0.05),
        { steps: 6 },
      );
      await g.page.mouse.up();
      await g.waitFor((inspection) =>
        cameraChanged(beforeOrbitAfterRight, inspection),
      );

      const assertGain = async (expected: number) => {
        const deadline = performance.now() + 10_000;
        for (;;) {
          const state = await g.call<GalleryGuiState>("galleryGuiState");
          const slider = control(state, gainSelector);
          const app = await guiApplication(g);
          if (
            slider.value.kind === "scalar" &&
            Math.abs(slider.value.value - expected) < 1e-6 &&
            Math.abs(app.state.gain - expected) < 1e-6
          )
            return;
          assert.ok(
            performance.now() < deadline,
            "gain endpoint did not settle",
          );
          await new Promise((resolve) => setTimeout(resolve, 25));
        }
      };
      const dragBeyondSlider = async (direction: "min" | "max") => {
        const target = await find(g, gainSelector.symbol);
        const [x, y, width, height] = target.bounds;
        const start = await point(gainSelector.symbol);
        const [end] = await projectContent(
          g,
          [
            [
              x + width / 2,
              y +
                height / 2 +
                (direction === "max" ? -1 : 1) * 1.3 * dialTravel,
            ],
          ],
          2 * (await spacing(g)),
        );
        assert.ok(end);
        await g.drag(
          [start.clientX, start.clientY],
          [end.clientX, end.clientY],
        );
      };
      const beforeMaximum = transform(await g.inspect());
      await dragBeyondSlider("max");
      await assertGain(1);
      assert.deepEqual(transform(await g.settle()), beforeMaximum);
      await assertGain(1);
      const beforeMinimum = transform(await g.inspect());
      await dragBeyondSlider("min");
      await assertGain(0);
      assert.deepEqual(transform(await g.settle()), beforeMinimum);
      await assertGain(0);

      const manualPose = transform(await g.inspect());
      await g.call("gallerySceneAction", "setExploded", true);
      await waitSpacing(g, (await guiApplication(g)).state.layerStep);
      assert.deepEqual(transform(await g.settle()), manualPose);
      await g.call("gallerySceneAction", "setExploded", false);
      await waitSpacing(g, 0);
      assert.deepEqual(transform(await g.settle()), manualPose);
      const beforeLogout = transform(await g.inspect());
      await press(g, await find(g, "gui-scanner-close"), 0);
      await waitApp(g, (app) => app.state.app.phase === "login");
      assert.deepEqual(
        transform(await g.settle()),
        beforeLogout,
        "logging out changed the manual camera pose",
      );
      await g.capture("camera-login-after-logout");
      const input = await point("gui-callsign");
      await unchangedAfterDrag(
        [input.clientX - span(0.014), input.clientY],
        [input.clientX + span(0.033), input.clientY],
      );
      await g.capture("camera-login-text-capture");

      edge = await panelEdge();
      await g.page.mouse.move(...edge.outside);
      await g.page.mouse.down();
      await g.page.evaluate(() => {
        document.querySelector<HTMLButtonElement>("#world-shapes")?.click();
      });
      await g.page.waitForFunction(
        () =>
          document.querySelector<HTMLElement>(".viewer-shell")?.dataset.page ===
          "shapes",
      );
      await g.page.mouse.move(
        edge.outside[0] + span(0.105),
        edge.outside[1] + span(0.047),
      );
      await g.page.mouse.up();
      const afterNavigation = transform(await g.settle());
      await new Promise((resolve) => setTimeout(resolve, 50));
      assert.deepEqual(transform(await g.inspect()), afterNavigation);
      assert.deepEqual(g.errors, []);
    },
  );
});
