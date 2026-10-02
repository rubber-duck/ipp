import type {
  RenderStatisticsSnapshot,
  SurfaceCacheRecord,
} from "@ipp/client/diagnostics";
import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import type { Inspection } from "@ipp/client";
import { runBrowserEnvironment } from "../browser/environment.js";
import {
  galleryEnvironment,
  openGallery,
  transform,
} from "./gallery-driver.js";
import {
  PANEL,
  control,
  controlPoint,
  controlRegion,
  gainFraction,
  projectContent,
  type ProjectedPoint,
} from "./gallery-gui-panel.js";
import type {
  GalleryGuiSelector,
  GalleryGuiState,
} from "./viewer-browser-helper.js";

const environment = {
  ...galleryEnvironment,
  evidenceParent: resolve("target/integration-artifacts/gallery-gui-camera"),
};

/** Gallery panel cache policy, restated independently of scene.tsx. */
const CACHE_DIRECT_DISTANCE = 20;
const CACHE_TEXELS_PER_METRE = 80;

/** Cache image size in a band; Surface sizes are f32 fields, rounded up with the renderer's 1e-4 texel tolerance. */
function expectedCacheSize(band: number): readonly [number, number] {
  const density = CACHE_TEXELS_PER_METRE / 2 ** (band - 1);
  return [
    Math.ceil(Math.fround(7.4) * density - 1e-4),
    Math.ceil(Math.fround(4.8) * density - 1e-4),
  ];
}

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

test("GUI demo routing owns panel gestures and admits background camera gestures", {
  timeout: 120_000,
}, async (context) => {
  await runBrowserEnvironment(
    "GUI camera input routing",
    environment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario, { initialPage: "gui" });
      await g.page.waitForFunction(
        () =>
          document.querySelector<HTMLElement>(".viewer-shell")?.dataset.page ===
            "gui" &&
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "ready",
      );
      // The station's first node sync and its toast end before counts
      // start: their progress and countdown change the panel's paint.
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-operation")?.textContent ===
          "Node sync: complete",
      );
      for (const deadline = performance.now() + 15_000; ; ) {
        const state = await g.call<GalleryGuiState>("galleryGuiState");
        const closes = state.controls.filter(({ symbol }) =>
          /^gui-toasts\/[^/]+\/close$/.test(symbol ?? ""),
        );
        if (closes.length === 0) break;
        if (closes.length === 1)
          try {
            await g.call(
              "galleryGuiAction",
              { role: "button", name: "Dismiss" },
              { kind: "press" },
            );
          } catch (failure) {
            // The toast may dismiss itself at the end of its time first.
            if (
              !(
                failure instanceof Error &&
                failure.message.includes("StaleTarget")
              )
            )
              throw failure;
          }
        assert.ok(performance.now() < deadline, "the station's toasts stayed");
        await new Promise((resolve) => setTimeout(resolve, 50));
      }
      await g.page.locator("#ipp-world-canvas").scrollIntoViewIfNeeded();
      const canvas = await g.page.locator("#ipp-world-canvas").boundingBox();
      assert.ok(canvas);
      // Pointer distances as a share of the canvas width, so gestures keep
      // their extent in the scene at any canvas size.
      const span = (share: number) => canvas.width * share;

      const point = (
        role: GalleryGuiSelector["role"],
        name?: string,
        x = 0.5,
      ) =>
        controlPoint(
          g,
          { role, ...(name === undefined ? {} : { name }) },
          x,
          0.5,
        );
      const panelEdge = async () => {
        const corners = await g.call<ProjectedPoint[]>(
          "projectGalleryPoints",
          "gui-demo",
          [
            [-3.7, 2.4, 0],
            [3.7, 2.4, 0],
            [-3.7, -2.4, 0],
            [3.7, -2.4, 0],
          ],
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

      // Camera-only motion over the cached panel composites the existing
      // image: no repaint and no upload. Static content makes counts exact.
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
      await g.call(
        "galleryGuiAction",
        { role: "checkbox", name: "SCAN" },
        { kind: "toggle" },
      );
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-autoscan")?.textContent === "standby",
      );
      await new Promise((resolve) => setTimeout(resolve, 550));
      assert.ok(panelDistance(await g.inspect()) < CACHE_DIRECT_DISTANCE);
      const near = await cacheUntil(
        "camera-cache-near",
        (record) => record?.mode === "near",
      );
      const bandOneDistance = await dollyOut(CACHE_DIRECT_DISTANCE * 1.2);
      assert.ok(bandOneDistance < 2 * CACHE_DIRECT_DISTANCE);
      const bandOne = await cacheUntil("camera-cache-band1", reusedBand(1));
      assert.equal(bandOne.allocations - near.allocations, 1);
      assert.equal(bandOne.repaints - near.repaints, 1);
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
      assert.deepEqual(
        [orbited.record?.width, orbited.record?.height],
        expectedCacheSize(1),
      );
      assert.equal(orbited.repaints - bandOne.repaints, 0);
      assert.equal(orbited.allocations - bandOne.allocations, 0);
      assert.equal(orbited.uploaded - bandOne.uploaded, 0);
      assert.equal(orbited.statistics!.surfaces!.surfaceCacheReuses, 1);
      assert.equal(orbited.statistics!.frame.uploadedBytes, 0);
      // Dollying past the next boundary resizes the same image once.
      edge = await panelEdge();
      await dollyOut(2 * CACHE_DIRECT_DISTANCE * 1.2);
      const bandTwo = await cacheUntil("camera-cache-band2", reusedBand(2));
      assert.equal(bandTwo.allocations - orbited.allocations, 1);
      assert.equal(bandTwo.repaints - orbited.repaints, 1);
      assert.deepEqual(
        [bandTwo.record?.width, bandTwo.record?.height],
        expectedCacheSize(2),
      );
      assert.equal(
        bandTwo.statistics!.surfaces!.surfaceCacheResidentBytes,
        4 * expectedCacheSize(2)[0] * expectedCacheSize(2)[1],
      );
      await g.page.locator("#reset-camera").click();
      await cacheUntil(
        "camera-cache-reset",
        (record) => record?.mode === "near",
      );
      await g.call(
        "galleryGuiAction",
        { role: "checkbox", name: "SCAN" },
        { kind: "toggle" },
      );
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-autoscan")?.textContent === "enabled",
      );

      const [titleX, titleY, titleWidth, titleHeight] = PANEL.telemetryTitle;
      const [header] = await projectContent(g, [
        [titleX + titleWidth / 2, titleY + titleHeight / 2],
      ]);
      assert.ok(header);
      edge = await panelEdge();
      await unchangedAfterDrag([header.clientX, header.clientY], edge.outside);

      const sliderStart = await point("slider", undefined, gainFraction(0.25));
      const sliderEnd = await point("slider", undefined, gainFraction(0.75));
      await unchangedAfterDrag(
        [sliderStart.clientX, sliderStart.clientY],
        [sliderEnd.clientX, sliderEnd.clientY],
      );

      // Keep a real browser pointer held while replaying a high-rate move
      // stream. Six Playwright drag steps did not expose the request backlog
      // caused by updating hundreds of waveform decoration nodes per gain edit.
      await g.call(
        "galleryGuiAction",
        { role: "slider" },
        { kind: "scalar", value: 0.1 },
      );
      const beforeStream = transform(await g.inspect());
      await g.capture("sustained-slider-before");
      const sliderRegion = await controlRegion(
        g,
        { role: "slider" },
        0.02,
        0.08,
      );
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

      // Each drag frame commits a slider value, and GUI input frames restore
      // animated values before admitting it. Neither may repaint or
      // re-upload panels the drag did not touch: per frame, rebuilds stay
      // within the slider's own background, fill and focus-ring boxes, and
      // uploads beyond the idle baseline come only from rebuilt batches: a
      // box rewrites one 192-byte record per quad and the value label
      // rewrites its glyph batch, one 64-byte record per glyph. Measured on
      // SwiftShader with separate shape and glyph records (ipp-sfgq.14, run
      // 2026-10-02): 0.56 rebuilds per drag frame at about 1.6 kB each, so
      // the bound below holds a margin of about two.
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
      assert.deepEqual(inputStream.errors, []);
      assert.equal(inputStream.completed, inputStream.sent);
      assert.deepEqual(transform(await g.settle()), beforeStream);
      await g.page.waitForFunction(
        () => document.querySelector("#gui-gain")?.textContent === "75%",
      );
      const streamed = await g.call<GalleryGuiState>("galleryGuiState");
      const gain = control(streamed, { role: "slider" }).value;
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

      const input = await point("text", "CALLSIGN");
      await unchangedAfterDrag(
        [input.clientX - span(0.014), input.clientY],
        [input.clientX + span(0.033), input.clientY],
      );

      const beforeQuickDrag = transform(await g.inspect());
      await g.drag(edge.outside, [
        edge.outside[0] + edge.tangent[0] * span(0.04),
        edge.outside[1] + edge.tangent[1] * span(0.04),
      ]);
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
      // No ScrollView consumes a wheel over the PULSE button, so the
      // runtime reports it unhandled and the camera zooms over the panel.
      const pulse = await point("button", "PULSE");
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

      const first = await point("button", "PULSE");
      const second = await point("button", "SYNC");
      // The semantic interaction state names the one hovered control; the
      // skin paints that state, so a stale hover would also keep its look.
      const hovered = async (name: string) =>
        control(await g.call<GalleryGuiState>("galleryGuiState"), {
          role: "button",
          name,
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
        if (await hovered("SYNC")) break;
        assert.ok(
          performance.now() - burstStarted < 550,
          "rapid hover burst retained a stale control",
        );
        await new Promise((resolve) => setTimeout(resolve, 16));
      }
      assert.equal(
        await hovered("PULSE"),
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
          const slider = control(state, { role: "slider" });
          const label = await g.page.locator("#gui-gain").textContent();
          if (
            slider.value.kind === "scalar" &&
            Math.abs(slider.value.value - expected) < 1e-6 &&
            label === `${Math.round(expected * 100)}%`
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
        const minimum = await point("slider", undefined, 0.08);
        const maximum = await point("slider", undefined, 0.92);
        const vector = [
          maximum.clientX - minimum.clientX,
          maximum.clientY - minimum.clientY,
        ] as const;
        const start = direction === "max" ? minimum : maximum;
        const end =
          direction === "max"
            ? ([
                maximum.clientX + vector[0] * 0.45,
                maximum.clientY + vector[1] * 0.45,
              ] as const)
            : ([
                minimum.clientX - vector[0] * 0.45,
                minimum.clientY - vector[1] * 0.45,
              ] as const);
        await g.drag([start.clientX, start.clientY], end);
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
