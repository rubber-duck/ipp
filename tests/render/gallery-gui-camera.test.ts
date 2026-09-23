import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import type {
  DynamicValue,
  GuiSemanticRole,
  GuiSemanticTree,
  Inspection,
  SurfaceCacheRecord,
} from "@ipp/client";
import { runBrowserEnvironment } from "../browser/environment.js";
import {
  galleryEnvironment,
  openGallery,
  transform,
} from "./gallery-driver.js";

interface ProjectedPoint {
  readonly clientX: number;
  readonly clientY: number;
}

interface GalleryGuiState {
  readonly semantic: GuiSemanticTree;
}

const environment = {
  ...galleryEnvironment,
  evidenceParent: resolve("target/reviews/gui-camera-input"),
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
      await g.call("galleryGuiState");
      await g.page.locator("#ipp-world-canvas").scrollIntoViewIfNeeded();

      const point = (role: GuiSemanticRole, name?: string, x = 0.5) =>
        g.call<ProjectedPoint>("galleryGuiPoint", { role, name }, x, 0.5);
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
        assert.ok(edge.room > 30, "GUI demo leaves no camera drag area");
        return {
          outside: [
            edge.midpoint[0] + edge.outward[0] * 14,
            edge.midpoint[1] + edge.outward[1] * 14,
          ] as [number, number],
          inside: [
            edge.midpoint[0] - edge.outward[0] * 14,
            edge.midpoint[1] - edge.outward[1] * 14,
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
        const records = frame.backend.surfaceCaches as
          | readonly SurfaceCacheRecord[]
          | undefined;
        assert.ok(records, "Surface cache diagnostics are unavailable");
        const { ingress: _ingress, ...stats } = frame.backend;
        await scenario.evidence.record(`${label}-surface-cache`, stats);
        return {
          record: records.find(({ entity }) => entity === panel),
          backend: frame.backend,
          repaints: Number(frame.backend.totalSurfaceCacheRepaints),
          allocations: Number(frame.backend.totalSurfaceCacheAllocations),
          uploaded: Number(frame.backend.totalUploadedBytes),
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
        { role: "checkbox" },
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
        edge.outside[0] + edge.tangent[0] * 60,
        edge.outside[1] + edge.tangent[1] * 60,
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
      assert.equal(orbited.backend.surfaceCacheReuses, 1);
      assert.equal(orbited.backend.uploadedBytes, 0);
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
        bandTwo.backend.surfaceCacheResidentBytes,
        4 * expectedCacheSize(2)[0] * expectedCacheSize(2)[1],
      );
      await g.page.locator("#reset-camera").click();
      await cacheUntil(
        "camera-cache-reset",
        (record) => record?.mode === "near",
      );
      await g.call(
        "galleryGuiAction",
        { role: "checkbox" },
        { kind: "toggle" },
      );
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-autoscan")?.textContent === "enabled",
      );

      const header = await point("text", "GUI DEMO");
      edge = await panelEdge();
      await unchangedAfterDrag([header.clientX, header.clientY], edge.outside);

      const sliderStart = await point("slider", undefined, 0.25);
      const sliderEnd = await point("slider", undefined, 0.75);
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
        { kind: "setScalar", value: 0.1 },
      );
      const beforeStream = transform(await g.inspect());
      await g.capture("sustained-slider-before");
      const sliderRegion = await g.call<
        readonly [number, number, number, number]
      >("galleryGuiRegion", { role: "slider" }, 0.02, 0.08);
      // Accumulated render work since the worker started, read at a frame.
      const renderTotals = async (label: string) => {
        const { frame } = await g.call<{
          frame: { tick: bigint; backend: Record<string, unknown> };
        }>("captureUnflushedViewer", label);
        return {
          tick: Number(frame.tick),
          rebuilds: Number(frame.backend.totalGuiRebuilds),
          uploaded: Number(frame.backend.totalUploadedBytes),
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
      // box rewrites six 152-byte vertices and the value label rewrites its
      // glyph batch, six such vertices per glyph. Measured on SwiftShader
      // (ipp-rm0k.25): 1.7 rebuilds per drag frame at about 1.8 kB each.
      const sliderBoxes = 3;
      const maxBoxUploadBytes = 4096;
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
      const gain = streamed.semantic.nodes.find(
        ({ role }) => role === "slider",
      )!.value;
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

      const input = await point("textInput", "CALLSIGN");
      await unchangedAfterDrag(
        [input.clientX - 12, input.clientY],
        [input.clientX + 28, input.clientY],
      );

      const beforeQuickDrag = transform(await g.inspect());
      await g.drag(edge.outside, [
        edge.outside[0] + edge.tangent[0] * 34,
        edge.outside[1] + edge.tangent[1] * 34,
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
      const pulse = await point("button", "PULSE");
      const beforePanelWheel = transform(await g.inspect());
      await g.page.mouse.move(pulse.clientX, pulse.clientY);
      await g.page.mouse.wheel(0, 120);
      assert.deepEqual(transform(await g.settle()), beforePanelWheel);

      const beforeOutsideWheel = transform(await g.inspect());
      await g.page.mouse.move(...edge.outside);
      await g.page.mouse.wheel(0, 120);
      await g.waitFor((inspection) =>
        cameraChanged(beforeOutsideWheel, inspection),
      );

      const aurora = await point("button", "AURORA");
      const ember = await point("button", "EMBER");
      const probeTree = (await g.call<GalleryGuiState>("galleryGuiState"))
        .semantic;
      const partScale = (name: string) =>
        g.call<DynamicValue>(
          "galleryGuiPartValue",
          probeTree.entity,
          probeTree.nodes.find(
            (node) => node.role === "button" && node.name === name,
          )!.id,
          "background",
          "scale",
        );
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
          [aurora.clientX, aurora.clientY],
          [ember.clientX, ember.clientY],
        ],
      );
      for (;;) {
        const finalScale = await partScale("EMBER");
        if (
          finalScale.kind === "vec2" &&
          Math.abs(finalScale.value[0] - 1.025) < 1e-4
        )
          break;
        assert.ok(
          performance.now() - burstStarted < 550,
          "rapid hover burst retained a stale control",
        );
        await new Promise((resolve) => setTimeout(resolve, 16));
      }
      const priorScale = await partScale("AURORA");
      assert.equal(priorScale.kind, "vec2");
      assert.ok(Math.abs(priorScale.value[0] - 1) < 1e-4);

      const assertGain = async (expected: number) => {
        const deadline = performance.now() + 10_000;
        for (;;) {
          const state = await g.call<GalleryGuiState>("galleryGuiState");
          const slider = state.semantic.nodes.find(
            ({ role }) => role === "slider",
          );
          const label = await g.page.locator("#gui-gain").textContent();
          if (
            slider?.value.kind === "scalar" &&
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
      await g.page.mouse.move(edge.outside[0] + 90, edge.outside[1] + 40);
      await g.page.mouse.up();
      const afterNavigation = transform(await g.settle());
      await new Promise((resolve) => setTimeout(resolve, 50));
      assert.deepEqual(transform(await g.inspect()), afterNavigation);
      assert.deepEqual(g.errors, []);
    },
  );
});
