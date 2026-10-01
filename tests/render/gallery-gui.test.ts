import type {
  AnimationControllerSnapshot,
  Inspection,
  RenderStatisticsSnapshot,
  SurfaceCacheRecord,
} from "@ipp/client";
import assert from "node:assert/strict";
import { join, resolve } from "node:path";
import { writeFile } from "node:fs/promises";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";
import { responseGate } from "../browser/response-gate.js";
import {
  galleryEnvironment,
  openGallery,
  transform,
} from "./gallery-driver.js";
import {
  compareFrames,
  count,
  differenceImage,
  encodePng,
  intersectionOverUnion,
  mask,
  pixelDifference,
  type RgbaFrame,
} from "./retained-gui-images.js";
import {
  FONT_METRICS,
  ICON_BOXES,
  ICON_CODE_POINTS,
  NOTES,
  NOTES_TEXT,
  PANEL,
  assertRetainedControls,
  control,
  controlPoint,
  controlRect,
  controlRegion,
  controlValue,
  logical,
  projectContent,
  telemetryScrollViews,
  wrapColumns,
  type ContentRect,
  type LogicalRect,
  type ProjectedPoint,
  type ScrollViews,
} from "./gallery-gui-panel.js";
import type {
  GalleryGuiSelector,
  GalleryGuiState,
  GalleryWaveform,
  GalleryWaveformTrace,
} from "./viewer-browser-helper.js";
import {
  PROJECTOR_MESH_SOURCES,
  PROJECTOR_TEXTURE_SOURCES,
} from "../../examples/world-gallery/worlds/gui/projector.js";

interface RegionStats {
  readonly pixels: number;
  readonly mean: readonly [number, number, number];
  readonly min: readonly [number, number, number];
  readonly max: readonly [number, number, number];
}

/** Symbolic IDs the gallery names, restated independently of the fixture. */
const PANEL_ENTITY = "gui-demo";
const PANEL_WORLD = "gui-demo-panel";

/** Aurora `disabled` background authored by the gallery control theme. */
const AURORA_DISABLED = [0.2, 0.35, 0.42, 0.45] as const;

/** The gallery's 0.16 s skin transition plus host-frame and capture
 * latency, matching the hover probe in gallery-gui-camera. */
const SKIN_SETTLE_MS = 550;

/**
 * One notch of the gallery's wheel: an eighth of the 0.78 telemetry
 * viewport, the step the gallery gives its GUI input.
 */
const WHEEL_STEP = 0.78 / 8;

/**
 * CSS pixels of one Chromium wheel notch. The browser adapter converts DOM
 * deltas to notches and scrolls each by the gallery's step, so the suites
 * dispatch whole notches.
 */
const WHEEL_NOTCH_PIXELS = 100;

/** Largest per-channel difference between two region means. */
function meanDifference(first: RegionStats, second: RegionStats): number {
  return Math.max(
    ...first.mean.map((value, channel) =>
      Math.abs(value - second.mean[channel]!),
    ),
  );
}

function srgbToLinear(value: number): number {
  const unit = value / 255;
  return unit <= 0.04045 ? unit / 12.92 : ((unit + 0.055) / 1.055) ** 2.4;
}

function linearToSrgb(value: number): number {
  const encoded =
    value <= 0.0031308 ? value * 12.92 : 1.055 * value ** (1 / 2.4) - 0.055;
  return encoded * 255;
}

function near(actual: number, expected: number, what: string): void {
  assert.ok(
    Math.abs(actual - expected) < 0.01,
    `${what}: ${actual} is not ${expected}`,
  );
}

const guiEnvironment = {
  ...galleryEnvironment,
  evidenceParent: resolve("target/integration-artifacts/gallery-gui"),
};

function guiEntity(inspection: Inspection) {
  return inspection.entities.find(
    ({ metadata }) => metadata.symbolicId === PANEL_ENTITY,
  );
}

function sceneEntity(inspection: Inspection, symbolicId: string) {
  const entity = inspection.entities.find(
    ({ metadata }) => metadata.symbolicId === symbolicId,
  );
  assert.ok(entity, `missing scene entity ${symbolicId}`);
  return entity;
}

function fieldsWith(inspection: Inspection, symbolicId: string, field: string) {
  const fields = sceneEntity(inspection, symbolicId).components.find(
    (entry) => field in entry.fields,
  )?.fields;
  assert.ok(fields, `${symbolicId} has no ${field} field`);
  return fields;
}

function dynamicProperty(
  inspection: Inspection,
  symbolicId: string,
  property: string,
) {
  const value = sceneEntity(inspection, symbolicId).components.find(
    (entry) => entry.properties && property in entry.properties,
  )?.properties?.[property];
  assert.ok(value, `${symbolicId} has no ${property} property`);
  return value;
}

function animationFor(inspection: Inspection, symbolicId: string) {
  const target = sceneEntity(inspection, symbolicId).id;
  const controller = inspection.controllers?.find((candidate) =>
    candidate.description.drivers.some((driver) => driver.target === target),
  );
  assert.ok(controller, `missing animation for ${symbolicId}`);
  return controller;
}

/** The controller animating one waveform trace in the panel World. */
function traceController(
  trace: GalleryWaveformTrace,
  pulse: boolean,
): AnimationControllerSnapshot {
  const controller = trace.controller;
  assert.ok(
    controller,
    `missing GUI waveform ${pulse ? "pulse" : "scan"} animation`,
  );
  assert.equal(controller.description.looping, !pulse);
  return controller;
}

const scan = (waveform: GalleryWaveform) =>
  traceController(waveform.scan, false);
const wavePulse = (waveform: GalleryWaveform) =>
  traceController(waveform.pulse, true);

function assertNoScanner(inspection: Inspection): void {
  assert.ok(
    !inspection.entities.some(
      ({ metadata }) => metadata.symbolicId === "gui-projector-scanner",
    ),
    "obsolete sweeping scanline is still mounted",
  );
}

function assertScalarValue(state: GalleryGuiState, expected: number) {
  const value = controlValue(state, "slider");
  assert.equal(value.kind, "scalar");
  assert.ok(Math.abs(value.value - expected) < 1e-6);
}

function assertCameraFov(inspection: Inspection, expected: number): void {
  const actual = Number(
    fieldsWith(inspection, "gallery-camera", "fov_y").fov_y,
  );
  assert.ok(
    Math.abs(actual - expected) < 1e-6,
    `camera FOV was ${actual}, expected ${expected}`,
  );
}

function documentStatus(value: string | null): string {
  return value?.trim() ?? "";
}

/** Controls must be staged hidden, and inert, until their resources load. */
function assertStaged(state: GalleryGuiState, message?: string) {
  assert.ok(state.controls.length > 0, "the panel declares no controls");
  assert.ok(
    state.controls.every(({ visible }) => !visible),
    message ?? "staged controls became visible while resources were pending",
  );
}

test("Gallery runs a real GUI demo and cleans it up", {
  timeout: 240_000,
}, async (context) => {
  const cancelFont = responseGate();
  const cancelMesh = responseGate();
  const readyFont = responseGate();
  const readyMesh = responseGate();
  const readyWaveform = responseGate();
  let responseMode: "failure" | "cancel" | "ready" | "pass" = "pass";
  const requests: { mode: string; path: string }[] = [];
  let failedRequest!: () => void;
  const failureRequested = new Promise<void>((resolve) => {
    failedRequest = resolve;
  });
  await runBrowserEnvironment(
    "GUI demo",
    guiEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario, {
        initialPage: "gui",
        beforeResponse: async (url, signal) => {
          requests.push({ mode: responseMode, path: url.pathname });
          const font = url.pathname.endsWith("/shure-tech-mono.ippf");
          const mesh = url.pathname.endsWith(PROJECTOR_MESH_SOURCES[0]);
          const waveform = url.pathname.endsWith("/waveform.ippd");
          if (waveform) {
            // The waveform drawing gates readiness like the font and mesh.
            if (responseMode === "ready") await readyWaveform.hold(signal);
            return undefined;
          }
          if (!font && !mesh) return;
          if (responseMode === "failure") {
            failedRequest();
            return { status: 503, body: "GUI demo resource unavailable\n" };
          }
          if (responseMode === "cancel") {
            await (font ? cancelFont : cancelMesh).hold(signal);
          } else if (responseMode === "ready") {
            await (font ? readyFont : readyMesh).hold(signal);
          }
          return undefined;
        },
      });
      // Held responses are awaited with a deadline; a missing request
      // fails with the requests the proxy saw.
      const requested = async (what: string, request: Promise<unknown>) => {
        let timer: ReturnType<typeof setTimeout> | undefined;
        try {
          await Promise.race([
            request,
            new Promise((_, reject) => {
              timer = setTimeout(
                () => reject(new Error(`${what} was never requested`)),
                15_000,
              );
            }),
          ]);
        } catch (failure) {
          await scenario.evidence.record("gui-demo-requests", requests);
          throw failure;
        } finally {
          clearTimeout(timer);
        }
      };
      const waveformEvidence: Record<string, unknown> = {};
      const recordWaveform = async (name: string, value: unknown) => {
        waveformEvidence[name] = value;
        await writeFile(
          join(scenario.evidence.directory, "waveform-evidence.json"),
          JSON.stringify(
            waveformEvidence,
            (_key, value) =>
              typeof value === "bigint" ? String(value) : value,
            2,
          ) + "\n",
        );
      };
      const waveform = (flush = true) =>
        g.call<GalleryWaveform>("galleryWaveform", flush);
      const waitForWaveform = async (
        predicate: (waveform: GalleryWaveform) => boolean,
      ) => {
        const deadline = performance.now() + 15_000;
        for (;;) {
          const current = await waveform();
          if (predicate(current)) return current;
          assert.ok(
            performance.now() < deadline,
            `GUI waveform did not settle: ${JSON.stringify(current, (_key, value) => (typeof value === "bigint" ? String(value) : value))}`,
          );
          await new Promise((resolve) => setTimeout(resolve, 25));
        }
      };
      // The creator-owned panel World is destroyed after its attachment's
      // declarations are cleaned up and the attachment detaches.
      const waitForPanelWorldGone = async (message: string) => {
        const deadline = performance.now() + 15_000;
        while (
          (await g.call<string[]>("galleryWorlds")).includes(PANEL_WORLD)
        ) {
          assert.ok(performance.now() < deadline, message);
          await new Promise((resolve) => setTimeout(resolve, 50));
        }
      };
      const panelInspection = (flush = true) =>
        g.call<Inspection>("inspectGalleryPanel", flush);
      const waitForPanel = async (
        predicate: (inspection: Inspection) => boolean,
      ) => {
        const deadline = performance.now() + 15_000;
        for (;;) {
          const inspection = await panelInspection();
          if (predicate(inspection)) return inspection;
          assert.ok(performance.now() < deadline, "panel World did not settle");
          await new Promise((resolve) => setTimeout(resolve, 25));
        }
      };
      const waitForGui = async (
        predicate: (state: GalleryGuiState) => boolean = () => true,
      ) => {
        const deadline = performance.now() + 15_000;
        let lastError: unknown;
        while (performance.now() < deadline) {
          try {
            const state = await g.call<GalleryGuiState>("galleryGuiState");
            if (predicate(state)) return state;
          } catch (failure) {
            lastError = failure;
          }
          await new Promise((resolve) => setTimeout(resolve, 25));
        }
        throw new Error(
          `GUI demo did not settle${lastError instanceof Error ? `: ${lastError.message}` : ""}`,
        );
      };
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
      const [gridX, gridY, gridWidth, gridHeight] = PANEL.waveform;
      const waveformRegion = () =>
        g.call<readonly [number, number, number, number]>(
          "galleryGuiContentRegion",
          logical(PANEL.waveform),
        );
      const waveformDifference = async (before: string, after: string) =>
        g.call<{
          changedPixels: number;
          changedFraction: number;
          meanAbsoluteChannelDifference: number;
        }>("compareViewerCaptureRegion", before, after, await waveformRegion());
      const outerWaveformPixels = async (label: string) => {
        const points: [number, number][] = [];
        for (let row = 1; row < 40; row++) {
          if (row >= 12 && row <= 28) continue;
          for (let column = 1; column < 100; column++)
            points.push([
              gridX + (gridWidth * column) / 100,
              gridY + (gridHeight * row) / 40,
            ]);
        }
        const samples = await g.call<readonly (readonly number[])[]>(
          "sampleGalleryGuiCapture",
          label,
          points,
        );
        return samples.filter(
          ([r, g, b]) => g! > 110 && b! > 125 && g! - r! > 12,
        ).length;
      };
      const sampleWaveformBaseline = async (label: string) => {
        const groups = [0.06, 0.15, 0.85, 0.94].map((fraction) =>
          [-0.02, 0, 0.02].map(
            (offset) =>
              [
                gridX + gridWidth * fraction,
                gridY + gridHeight / 2 + offset,
              ] as const,
          ),
        );
        const pixels = await g.call<readonly (readonly number[])[]>(
          "sampleGalleryGuiCapture",
          label,
          groups.flat(),
        );
        return groups.map((_, group) =>
          Math.max(
            ...pixels.slice(group * 3, group * 3 + 3).map((pixel) => pixel[1]!),
          ),
        );
      };
      const sampleSineExtrema = async (label: string) => {
        const state = await waitForGui();
        const gain = controlValue(state, "slider");
        assert.equal(gain.kind, "scalar");
        const amplitude = ((20 * gridWidth) / 330) * (0.12 + 0.88 * gain.value);
        // Two complete sine cycles at phase zero: alternating crests and troughs.
        const fractions = [0.125, 0.375, 0.625, 0.875];
        const samples = await g.call<readonly (readonly number[])[]>(
          "sampleGalleryGuiCapture",
          label,
          fractions.flatMap((fraction) =>
            [-0.015, 0, 0.015].map((offset) => [
              gridX + gridWidth * fraction,
              gridY +
                gridHeight / 2 -
                amplitude * Math.sin(4 * Math.PI * fraction) +
                offset,
            ]),
          ),
        );
        return fractions.map((_, index) =>
          Math.max(
            ...samples
              .slice(index * 3, index * 3 + 3)
              .map((pixel) => pixel[1]!),
          ),
        );
      };
      const assertTransparentCorners = async (label: string) => {
        await g.call("overrideGalleryGuiTransform", { x: 1000 });
        try {
          await g.capture(`${label}-backdrop`);
        } finally {
          await g.call("releaseGalleryGuiTransform");
        }
        await g.capture(label);
        const corners = [
          [0.02, 0.02],
          [7.38, 0.02],
          [7.38, 4.78],
          [0.02, 4.78],
        ];
        const painted = await g.call<number[][]>(
          "sampleGalleryGuiCapture",
          label,
          corners,
        );
        const backdrop = await g.call<number[][]>(
          "sampleGalleryGuiCapture",
          `${label}-backdrop`,
          corners,
        );
        assert.ok(
          painted.every((pixel, index) =>
            pixel
              .slice(0, 3)
              .every(
                (value, channel) =>
                  Math.abs(value - backdrop[index]![channel]!) <= 2,
              ),
          ),
          `outside rounded panel corners must reveal the unchanged scene: ${JSON.stringify({ painted, backdrop })}`,
        );
      };
      // Region measurements and their expected values collect in one review
      // file beside waveform-evidence.json.
      const regionEvidence: Record<string, unknown> = {};
      const recordRegions = async (name: string, value: unknown) => {
        regionEvidence[name] = value;
        await writeFile(
          join(scenario.evidence.directory, "region-evidence.json"),
          JSON.stringify(regionEvidence, null, 2) + "\n",
        );
      };
      const regionStats = <K extends string>(
        label: string,
        rects: Record<K, LogicalRect>,
      ) =>
        g.call<Record<K, RegionStats>>("galleryGuiRegionStats", label, rects);
      // Face the panel to the camera so thin skin features span several
      // pixels, then restore the authored placement.
      const withDetailView = async <T>(body: () => Promise<T>) => {
        await g.page.mouse.move(1, 1);
        await g.call("faceGalleryGuiToCamera");
        try {
          return await body();
        } finally {
          await g.call("releaseGalleryGuiTransform");
        }
      };
      // UPLINK fill beside its label and the panel gap to its right.
      const uplinkRegions = (label: string) => {
        const [x, y, width, height] = PANEL.uplink;
        return regionStats(label, {
          fill: [x + 0.72, y + 0.12, x + 0.9, y + height - 0.12],
          gap: [
            x + width + 0.04,
            y + 0.12,
            x + width + 0.09,
            y + height - 0.12,
          ],
        });
      };
      // One detail capture after the skin transition has had time to
      // settle; a lane that stays on its previous sample fails the caller's
      // colour comparison instead of being retried.
      const settledUplink = (label: string) =>
        withDetailView(async () => {
          await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));
          await g.capture(label);
          return uplinkRegions(label);
        });
      const startGui = async () => {
        await g.selectScene("gui");
        await g.page.waitForFunction(
          () =>
            document.querySelector<HTMLElement>(".viewer-shell")?.dataset
              .page === "gui",
        );
      };
      await g.page.waitForFunction(
        () =>
          document.querySelector<HTMLElement>(".viewer-shell")?.dataset.page ===
            "gui" &&
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "ready",
      );
      const coldDirect = await g.capture("gui-demo-cold-direct");
      assert.equal(coldDirect.frame.failedDrawCalls, 0);
      assertCameraFov(coldDirect.inspection, (21 * Math.PI) / 180);
      const backdrop = await g.call<number[][]>(
        "sampleViewerCapture",
        "gui-demo-cold-direct",
        [
          [0.04, 0.04],
          [0.04, 0.45],
        ],
      );
      assert.ok(
        backdrop.every(
          ([r, g, b]) =>
            r! > 40 &&
            b! < 110 &&
            Math.max(r!, g!, b!) - Math.min(r!, g!, b!) < 16,
        ),
        `studio backdrop must paint soft gray: ${JSON.stringify(backdrop)}`,
      );
      assert.ok(
        backdrop[0]![0]! > backdrop[1]![0]! + 3,
        "studio backdrop lost its vertical gradient",
      );
      await assertTransparentCorners("gui-demo-rounded-corners");
      for (const [kind, sources] of [
        [1, PROJECTOR_MESH_SOURCES],
        [2, PROJECTOR_TEXTURE_SOURCES],
      ] as const) {
        for (const source of sources) {
          assert.ok(
            coldDirect.inspection.resources.some(
              (resource) =>
                resource.kind === kind &&
                resource.source.endsWith(source) &&
                resource.status === "loaded",
            ),
            `cold direct entry did not load ${source}`,
          );
        }
      }
      await g.page.screenshot({
        path: join(
          scenario.evidence.directory,
          "gui-demo-cold-direct-page.png",
        ),
        fullPage: true,
      });
      const coldSources = new Set(
        coldDirect.inspection.resources
          .filter(({ source }) => !source.startsWith("ipp://"))
          .map(({ source }) => source),
      );
      await g.navigate("shapes");
      await g.waitFor(
        (inspection) =>
          guiEntity(inspection) === undefined &&
          inspection.resources.every(({ source }) => !coldSources.has(source)),
      );
      await waitForPanelWorldGone("navigation retained the panel World");

      // The authored Host keeps unused completed assets, so re-entering the
      // GUI page reuses them without another request.
      const guiFetches = (from: number) =>
        requests
          .slice(from)
          .filter(
            ({ path }) =>
              path.endsWith("/shure-tech-mono.ippf") ||
              path.endsWith(PROJECTOR_MESH_SOURCES[0]) ||
              path.endsWith("/waveform.ippd"),
          );
      const reentryStart = requests.length;
      await g.navigate("gui");
      assert.deepEqual(
        guiFetches(reentryStart),
        [],
        "re-entering the GUI page fetched cached assets again",
      );
      await g.navigate("shapes");
      await waitForPanelWorldGone("navigation retained the panel World");

      // The failure, cancel and ready phases observe actual loads: reloading
      // the shapes page starts the authored canvas with a cold Host.
      let baseline!: { session: bigint };
      let baselineSources = new Set<string>();
      const coldAuthoredHost = async () => {
        await g.page.reload();
        await g.call("waitForViewer");
        await g.page.waitForFunction(
          () =>
            document.querySelector<HTMLElement>(".viewer-shell")?.dataset
              .page === "shapes" &&
            document.querySelector<HTMLOutputElement>("#status")?.dataset
              .state === "ready",
        );
        baseline = await g.call<{ session: bigint }>("observeViewer");
        const baselineInspection = await g.inspect();
        assertCameraFov(baselineInspection, Math.PI / 4);
        baselineSources = new Set(
          baselineInspection.resources.map(({ source }) => source),
        );
      };
      await coldAuthoredHost();
      responseMode = "failure";
      const guiSourcesGone = async (inspection: Inspection) =>
        guiEntity(inspection) === undefined &&
        inspection.resources.every(({ source }) =>
          baselineSources.has(source),
        ) &&
        !(await g.call<string[]>("galleryWorlds")).includes(PANEL_WORLD);

      const waitForGuiSourcesGone = async () => {
        const deadline = performance.now() + 15_000;
        for (;;) {
          if (await guiSourcesGone(await g.inspect())) return;
          assert.ok(
            performance.now() < deadline,
            "the GUI demo sources were not released",
          );
        }
      };

      await startGui();
      await requested("a failing resource", failureRequested);
      await g.page.waitForFunction(
        () =>
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "error" &&
          document.querySelector("#gui-loading[role='alert']"),
      );
      assert.equal(
        documentStatus(await g.page.locator("#gui-status").textContent()),
        "error",
      );
      await g.waitFor((inspection) => guiEntity(inspection) === undefined);
      await g.navigate("shapes");
      await waitForGuiSourcesGone();

      // Resources that completed during the failed start would be cached.
      await coldAuthoredHost();
      responseMode = "cancel";
      await startGui();
      await requested(
        "the cancelled font and mesh",
        Promise.all([cancelFont.requested, cancelMesh.requested]),
      );
      assertStaged(await waitForGui());
      const cancelledInspection = await g.inspect();
      assert.ok(
        Number(fieldsWith(cancelledInspection, PANEL_ENTITY, "qx").x) > 900,
      );
      assert.ok(
        Number(fieldsWith(cancelledInspection, "gui-projector-core", "qx").x) >
          900,
        "the 3D projector was not staged with its pending panel",
      );
      assert.equal(
        documentStatus(await g.page.locator("#gui-status").textContent()),
        "loading",
      );
      await g.navigate("shapes");
      await Promise.all([cancelFont.aborted, cancelMesh.aborted]);
      cancelFont.release();
      cancelMesh.release();
      await waitForGuiSourcesGone();

      await coldAuthoredHost();
      responseMode = "ready";
      const startupStarted = performance.now();
      await startGui();
      await requested(
        "the held font and mesh",
        Promise.all([readyFont.requested, readyMesh.requested]),
      );
      assert.equal(
        await g.page.getByRole("button", { name: "Reset camera" }).count(),
        1,
      );
      assertStaged(await waitForGui());
      const loadingFrame = await g.capturePending("gui-demo-loading");
      assertNoScanner(loadingFrame.inspection);
      const loadingWaveform = await waveform();
      assert.notEqual(scan(loadingWaveform).state, "playing");
      assert.notEqual(wavePulse(loadingWaveform).state, "playing");
      assert.notEqual(
        animationFor(loadingFrame.inspection, "gui-projector-beam").state,
        "playing",
      );

      assert.ok(
        loadingFrame.summary.coverage < 0.02,
        "the off-screen staging GUI demo leaked into the loading frame",
      );
      await g.page.screenshot({
        path: join(scenario.evidence.directory, "gui-demo-loading-page.png"),
      });
      assert.equal(
        await g.page.locator("#gui-loading").getAttribute("role"),
        "status",
      );
      assert.equal(
        documentStatus(
          await g.page.locator("#status").getAttribute("data-state"),
        ),
        "starting",
      );

      const fontLoaded = (inspection: Inspection) =>
        inspection.resources.some(
          ({ source, status }) =>
            source.endsWith("/shure-tech-mono.ippf") && status === "loaded",
        );
      readyMesh.release();
      await g.waitFor((inspection) =>
        inspection.resources.some(
          ({ source, status }) =>
            source.endsWith(PROJECTOR_MESH_SOURCES[0]) && status === "loaded",
        ),
      );
      assert.ok(
        !fontLoaded(await panelInspection()),
        "the held font loaded early",
      );
      readyFont.release();
      await requested("the held waveform drawing", readyWaveform.requested);
      await waitForPanel(fontLoaded);
      assertStaged(
        await waitForGui(),
        "the GUI demo prepared before its essential waveform drawing loaded",
      );
      assert.equal(
        await g.page.locator("#status").getAttribute("data-state"),
        "starting",
      );
      // The reveal moves the prepared panel and projector from their staging
      // position in one React render of the gallery World: its Transform
      // field writes. Hold them before they reach the Host.
      await g.call("delayNextBatchSubmission", "setField", "Transform");
      readyWaveform.release();
      await g.held();
      // The held render keeps the canvas from flushing: observe the Worlds
      // without waiting for it.
      const [resourcesReady, panelReady] = await Promise.all([
        g.inspect(),
        panelInspection(false),
      ]);
      const essentialResources = [
        ...panelReady.resources.filter(
          ({ source }) =>
            source.endsWith("/shure-tech-mono.ippf") ||
            source.endsWith("/waveform.ippd"),
        ),
        ...resourcesReady.resources.filter(({ source }) =>
          source.endsWith(PROJECTOR_MESH_SOURCES[0]),
        ),
      ];
      assert.equal(essentialResources.length, 3);
      assert.ok(essentialResources.every(({ status }) => status === "loaded"));

      assertNoScanner(resourcesReady);
      assert.notEqual(scan(await waveform(false)).state, "playing");
      const prepared = await g.call<GalleryGuiState>("galleryGuiState", false);
      assert.ok(
        prepared.controls.every(({ visible }) => visible),
        "the held reveal did not follow the prepared panel controls",
      );
      assert.ok(
        Number(fieldsWith(resourcesReady, PANEL_ENTITY, "qx").x) > 900,
        "the GUI demo moved on-screen before its reveal",
      );
      const preparedFrame = await g.call<{ summary: { coverage: number } }>(
        "captureUnflushedViewer",
        "gui-demo-prepared-offscreen",
      );
      assert.ok(
        preparedFrame.summary.coverage < 0.02,
        "prepared GUI content appeared before GUI demo placement",
      );
      await scenario.evidence.record(
        "gui-demo-prepared-offscreen",
        preparedFrame,
      );
      const canvasBounds = await g.page
        .locator("#ipp-world-canvas")
        .boundingBox();
      assert.ok(canvasBounds);
      await g.page.mouse.click(
        canvasBounds.x + canvasBounds.width / 2,
        canvasBounds.y + canvasBounds.height / 2,
      );
      await g.settle();
      assert.equal(
        documentStatus(await g.page.locator("#gui-autoscan").textContent()),
        "enabled",
        "staged GUI demo accepted pointer input before placement",
      );
      const resourcesLoadedMs = performance.now() - startupStarted;
      assert.equal(
        await g.page.locator("#status").getAttribute("data-state"),
        "starting",
        "resource readiness bypassed the reveal acknowledgement",
      );
      await g.call("delayNextPresentedFrame");
      await g.call("releaseQuery");
      await g.page.waitForFunction(async (helper) => {
        const fixture = await import(helper);
        return fixture.presentedFrameHeld();
      }, g.helper);
      assert.equal(
        await g.page.locator("#status").getAttribute("data-state"),
        "starting",
        "the GUI demo reported ready before its first completed visible frame",
      );
      await g.call("releasePresentedFrame");
      await g.page.waitForFunction(
        () =>
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "ready",
      );
      await scenario.evidence.record("gui-demo-startup-timing", {
        resourcesLoadedMs,
        firstReadyMs: performance.now() - startupStarted,
        resources: essentialResources.map(({ kind, source, status }) => ({
          kind,
          source,
          status,
        })),
      });
      responseMode = "pass";

      await g.page.waitForFunction(() => {
        const fps = document.querySelector<HTMLOutputElement>("#canvas-fps");
        return (
          fps?.dataset.state === "live" &&
          /^Canvas FPS [1-9]\d*$/.test(fps.textContent?.trim() ?? "")
        );
      });
      await g.waitFor((inspection) => guiEntity(inspection) !== undefined);
      const initial = await waitForGui(
        ({ rows, controls }) =>
          rows >= 30 &&
          controls.length > 0 &&
          controls.every(({ available }) => available),
      );
      const firstReadyFrame = await g.capture("gui-demo-first-ready");
      assert.ok(firstReadyFrame.summary.coverage > 0.08);
      // The panel is ordinary Canvas content of an attached World presented
      // through the parent's Surface.
      assert.ok(initial.panelComponents.includes("Surface"));
      assert.ok(initial.panelComponents.includes("WorldAttachment"));
      const panelFills = initial.boxes.filter(({ symbol }) =>
        symbol?.endsWith("-frame/fill"),
      );
      assert.ok(panelFills.length >= 3);
      assert.ok(
        panelFills.every(
          ({ alpha, opacity }) => Math.abs(alpha * opacity - 0.9) < 1e-6,
        ),
        "each panel fill must paint at effective alpha 0.9",
      );
      // The event log VirtualList nests inside the telemetry ScrollView.
      telemetryScrollViews(initial);

      const buttons = initial.controls.filter(({ kind }) => kind === "button");
      assert.deepEqual(buttons.map(({ label }) => label).sort(), [
        "AURORA",
        "EMBER",
        "NEON",
        "PULSE",
        "PURGE",
        "SPAN",
        "UPLINK",
      ]);
      assert.deepEqual(controlValue(initial, "checkbox"), {
        kind: "bool",
        value: true,
      });
      assertScalarValue(initial, 0.64);
      assert.deepEqual(controlValue(initial, "text", "CALLSIGN"), {
        kind: "text",
        value: "VESPER-7",
      });
      for (const node of initial.controls) {
        assert.equal(node.visible, true);
        assert.equal(node.available, true);
        assert.equal(node.enabled, node.label !== "UPLINK");
      }

      const overview = await g.capture("gui-demo-overview");
      const panelPaint = await g.call<number[][]>(
        "sampleGalleryGuiCapture",
        "gui-demo-overview",
        [
          [3.7, 0.12],
          [3.7, 4.66],
          [0.15, 2.4],
          [7.22, 2.4],
        ],
      );
      assert.ok(
        panelPaint.every(
          ([r, g, b]) => r! < 55 && g! < 90 && b! < 100 && b! > r! + 10,
        ),
        `panel paint lost its dark translucent navy fill: ${JSON.stringify(panelPaint)}`,
      );
      const overviewInspection = overview.inspection;
      for (const symbol of [
        "gui-projector-core",
        "gui-projector-trim",
        "gui-projector-emitter",
        "gui-projector-beam",
        "gui-projector-base",
        "gui-projector-floor",
        "gui-projector-background",
        "gui-projector-light",
      ]) {
        sceneEntity(overviewInspection, symbol);
      }
      assert.ok(
        Number(fieldsWith(overviewInspection, "gui-projector-core", "qx").x) <
          1,
        "the projector did not leave its offscreen staging position",
      );
      const overviewWaveform = await waveform();
      assert.equal(scan(overviewWaveform).state, "playing");
      assertNoScanner(overviewInspection);
      const initialScan = scan(overviewWaveform);
      await waitForWaveform((current) => {
        const time = scan(current).time;
        return (time - initialScan.time + 2.4) % 2.4 > 0.45;
      });
      await g.capture("gui-waveform-advancing");
      assert.ok(
        (
          await waveformDifference(
            "gui-demo-overview",
            "gui-waveform-advancing",
          )
        ).changedPixels > 40,
        "playing SCAN did not move the curve inside its grid",
      );
      const initialProjectorIntensity = Number(
        fieldsWith(overviewInspection, "gui-projector-light", "intensity")
          .intensity,
      );
      const surfaceSources = new Set(
        overview.inspection.resources
          .map(({ source }) => source)
          .filter((source) => !baselineSources.has(source)),
      );
      const overviewPanel = await panelInspection();
      assert.ok(
        overviewPanel.resources.some(
          ({ kind, source, status }) =>
            kind === 17 &&
            source.endsWith("/shure-tech-mono.ippf") &&
            status === "loaded",
        ),
      );
      assert.deepEqual(
        overviewPanel.resources
          .filter(({ kind }) => kind === 18)
          .map(({ source }) => source.split("/").pop())
          .sort(),
        ["waveform-grid.ippd", "waveform-pulse.ippd", "waveform.ippd"],
        "complex waveform curves share Surface drawing resources",
      );
      assert.equal(
        initial.drawings.length,
        3,
        "waveform geometry must not expand into hundreds of Canvas entities",
      );
      assert.ok(Number(overview.frame.statistics!.gui!.guiBatches) > 0);
      assert.ok(Number(overview.frame.statistics!.gui!.glyphPages) > 0);
      const glyphTexts = initial.texts.map(({ text }) => text);
      for (const icon of Object.values(ICON_CODE_POINTS))
        assert.ok(glyphTexts.includes(icon), `missing Nerd Font icon ${icon}`);

      const motionClips = new Set(
        [...overview.inspection.resources, ...overviewPanel.resources]
          .filter(
            ({ kind, source, status }) =>
              kind === 10 &&
              source.includes("generated-") &&
              status === "loaded",
          )
          .map(({ source }) => source),
      );
      assert.equal(
        motionClips.size,
        6,
        "three skins, waveform and projector motion clips must all be resident",
      );
      assert.ok(overview.frame.drawCalls > 0 && overview.frame.triangles > 0);
      assert.equal(overview.frame.failedDrawCalls, 0);
      assert.ok(overview.summary.coverage > 0.08);
      assert.deepEqual(overview.inspection.renderDiagnostics, []);
      assert.deepEqual(overviewPanel.renderDiagnostics, []);

      // UPLINK is mounted disabled while SCAN runs. Its disabled state is an
      // ordinary theme part row and paints a solid dim fill.
      const disabledUplink = control(initial, {
        role: "button",
        name: "UPLINK",
      });
      assert.equal(disabledUplink.enabled, false);
      const disabledRow = await g.call<Record<string, unknown>>(
        "galleryGuiThemeRow",
        { role: "button", name: "UPLINK" },
        { part: "background", state: "disabled" },
      );
      (disabledRow.color as readonly number[]).forEach((value, channel) =>
        near(value, AURORA_DISABLED[channel]!, "disabled UPLINK colour"),
      );
      near(Number(disabledRow.opacity), 0.45, "disabled UPLINK opacity");
      assert.equal(disabledRow.fill_mode, 0, "disabled UPLINK is not solid");
      const disabledRegions = await settledUplink("gui-detail-uplink-disabled");
      // Straight linear colour at alpha 0.45 x opacity 0.45 over the
      // measured panel background, encoded once for display.
      const disabledAlpha = AURORA_DISABLED[3] * 0.45;
      const expectedDisabled = AURORA_DISABLED.slice(0, 3).map(
        (value, channel) =>
          linearToSrgb(
            value * disabledAlpha +
              srgbToLinear(disabledRegions.gap.mean[channel]!) *
                (1 - disabledAlpha),
          ),
      );
      await recordRegions("uplink-disabled", {
        ...disabledRegions,
        expectedDisabled,
      });
      disabledRegions.fill.mean.forEach((value, channel) =>
        assert.ok(
          Math.abs(value - expectedDisabled[channel]!) < 12,
          `disabled UPLINK fill ${JSON.stringify(disabledRegions.fill.mean)} is not ${JSON.stringify(expectedDisabled)}`,
        ),
      );

      const cameraBeforeControls = transform(await g.inspect());
      const checkboxRegion = await controlRegion(g, { role: "checkbox" });
      const sliderRegion = await controlRegion(
        g,
        { role: "slider" },
        0.02,
        0.08,
      );
      const checkbox = await point("checkbox");
      await g.page.mouse.click(checkbox.clientX, checkbox.clientY);
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-autoscan")?.textContent === "standby",
      );
      let current = await waitForGui((state) => {
        const value = controlValue(state, "checkbox");
        return value.kind === "bool" && value.value === false;
      });
      await waitForWaveform((current) => scan(current).state === "paused");

      // SCAN standby enables UPLINK; the skin transition returns its part
      // properties to the idle lane and the captured fill changes with them.
      current = await waitForGui(
        (state) => control(state, { role: "button", name: "UPLINK" }).enabled,
      );
      const enabledUplink = control(current, {
        role: "button",
        name: "UPLINK",
      });
      assert.equal(enabledUplink.target.entity, disabledUplink.target.entity);
      assert.equal(
        enabledUplink.target.incarnation,
        disabledUplink.target.incarnation,
      );
      const enabledRegions = await settledUplink("gui-detail-uplink-enabled");
      await recordRegions("uplink-enabled", enabledRegions);
      assert.ok(
        meanDifference(enabledRegions.fill, disabledRegions.fill) > 8,
        "enabling UPLINK did not change its captured fill",
      );
      assert.ok(
        meanDifference(enabledRegions.gap, disabledRegions.gap) < 4,
        "UPLINK comparison background moved between captures",
      );

      const dustStart = await g.capture("gui-projector-dust-start");
      const dustController = animationFor(
        dustStart.inspection,
        "gui-projector-beam",
      );
      assert.equal(
        dustController.state,
        "playing",
        "SCAN must not pause the ambient dust",
      );
      const initialPhase = dynamicProperty(
        dustStart.inspection,
        "gui-projector-beam",
        "phase",
      );
      assert.equal(initialPhase.kind, "f32");
      await g.waitFor(
        (inspection) =>
          Number(
            dynamicProperty(inspection, "gui-projector-beam", "phase").value,
          ) >
          Number(initialPhase.value) + 0.3,
      );
      await g.capture("gui-projector-dust-drifted");
      const dustDifference = await g.call<{ changedPixels: number }>(
        "compareViewerCaptureRegion",
        "gui-projector-dust-start",
        "gui-projector-dust-drifted",
        [0.44, 0.24, 0.535, 0.79],
      );
      assert.ok(
        dustDifference.changedPixels > 3,
        "Host-owned dust drift did not change the visible projection volume",
      );

      const pausedScan = scan(await waveform());
      assert.equal(pausedScan.id, initialScan.id);
      await g.capture("gui-waveform-paused");
      assert.ok(
        (
          await waveformDifference(
            "gui-projector-dust-start",
            "gui-waveform-paused",
          )
        ).changedPixels < 12,
        "the SCAN curve moved while paused",
      );
      assert.equal(scan(await waveform()).time, pausedScan.time);
      await g.call(
        "controlGalleryAnimation",
        pausedScan.id,
        { action: "seek", time: 0 },
        true,
      );
      await g.capture("gui-waveform-loop-start");
      await g.call(
        "controlGalleryAnimation",
        pausedScan.id,
        { action: "seek", time: 2.399999 },
        true,
      );
      await g.capture("gui-waveform-loop-end");
      const loopEnd = await waveform();
      assert.ok(
        loopEnd.scan.x < -3.5,
        "SCAN must travel left through the second authored tile",
      );
      const seamDifference = await waveformDifference(
        "gui-waveform-loop-start",
        "gui-waveform-loop-end",
      );
      await recordWaveform("seam", {
        difference: seamDifference,
        position: loopEnd.scan.x,
        controller: scan(loopEnd),
      });
      // Clipped curve endpoints and subpixel coverage can differ between tiles.
      // Bound both the affected area and total contrast, not raw pixel density.
      assert.ok(
        seamDifference.changedFraction < 0.005 &&
          seamDifference.meanAbsoluteChannelDifference < 0.25,
        `the periodic curve visibly jumps at its loop seam: ${JSON.stringify(seamDifference)}`,
      );
      await g.call(
        "controlGalleryAnimation",
        pausedScan.id,
        { action: "seek", time: 0 },
        true,
      );
      await g.call(
        "galleryGuiAction",
        { role: "slider" },
        { kind: "scalar", value: 0.1 },
      );
      await waitForGui((state) => {
        const value = controlValue(state, "slider");
        return value.kind === "scalar" && Math.abs(value.value - 0.1) < 1e-6;
      });
      await g.page.waitForFunction(
        () => document.querySelector("#gui-gain")?.textContent === "10%",
      );
      await g.capture("gui-waveform-low-gain");
      const sliderStart = await point("slider", undefined, 0.18);
      const sliderEnd = await point("slider", undefined, 0.84);
      await g.drag(
        [sliderStart.clientX, sliderStart.clientY],
        [sliderEnd.clientX, sliderEnd.clientY],
      );
      await g.page.waitForFunction(
        () =>
          Number.parseInt(
            document.querySelector("#gui-gain")?.textContent ?? "0",
          ) >= 75,
      );
      current = await waitForGui((state) => {
        const value = controlValue(state, "slider");
        return value.kind === "scalar" && value.value >= 0.75;
      });
      await g.waitFor(
        (inspection) =>
          Number(
            fieldsWith(inspection, "gui-projector-light", "intensity")
              .intensity,
          ) >
          initialProjectorIntensity * 1.1,
      );

      await g.capture("gui-waveform-high-gain");
      assert.ok(
        (
          await waveformDifference(
            "gui-waveform-low-gain",
            "gui-waveform-high-gain",
          )
        ).changedPixels > 35,
        "gain did not visibly change the paused waveform amplitude",
      );
      const lowAmplitudePixels = await outerWaveformPixels(
        "gui-waveform-low-gain",
      );
      const highAmplitudePixels = await outerWaveformPixels(
        "gui-waveform-high-gain",
      );
      await recordWaveform("gain", { lowAmplitudePixels, highAmplitudePixels });
      assert.ok(
        highAmplitudePixels > lowAmplitudePixels + 12,
        `higher gain must extend the visible curve above and below its center: ${JSON.stringify({ lowAmplitudePixels, highAmplitudePixels })}`,
      );
      const sineExtrema = await sampleSineExtrema("gui-waveform-high-gain");
      await recordWaveform("sine", { extrema: sineExtrema });
      assert.ok(
        sineExtrema.every((green) => green > 100),
        `SCAN must paint two smooth sine cycles across the graph: ${JSON.stringify(sineExtrema)}`,
      );
      assert.equal(
        scan(await waveform()).id,
        initialScan.id,
        "gain replaced the waveform scan controller",
      );
      const identityBeforeCamera = current;
      await g.capture("gui-demo-before-camera");
      assert.equal(
        await g.page
          .getByRole("button", { name: /^(Rotate|Tilt|Zoom) / })
          .count(),
        0,
        "GUI camera navigation should use background gestures",
      );
      const cameraCanvasBounds = await g.page
        .locator("#ipp-world-canvas")
        .boundingBox();
      assert.ok(cameraCanvasBounds);
      const cameraDrag = [
        cameraCanvasBounds.x + 20,
        cameraCanvasBounds.y + 20,
      ] as const;
      await g.drag(cameraDrag, [cameraDrag[0] + 36, cameraDrag[1] + 24]);
      await g.waitFor(
        (inspection) =>
          Math.abs(
            Number(transform(inspection).qy) - Number(cameraBeforeControls.qy),
          ) > 0.01,
      );
      await g.page.mouse.move(...cameraDrag);
      await g.page.mouse.wheel(0, -80);
      await g.settle();
      const cameraOblique = transform(await g.inspect());
      assert.notDeepEqual(
        cameraOblique,
        cameraBeforeControls,
        "background gestures did not move the GUI demo camera",
      );
      assertRetainedControls(identityBeforeCamera, await waitForGui());
      const obliqueFrame = await g.capture("gui-demo-camera-oblique");
      await assertTransparentCorners("gui-demo-rounded-corners-oblique");
      await g.page.screenshot({
        path: join(
          scenario.evidence.directory,
          "gui-demo-camera-oblique-page.png",
        ),
        fullPage: true,
      });
      assert.equal(obliqueFrame.frame.failedDrawCalls, 0);
      assert.ok(
        (
          await g.difference(
            "gui-demo-before-camera",
            "gui-demo-camera-oblique",
          )
        ).changedPixels > 1_000,
        "camera gestures did not visibly change the completed GUI demo frame",
      );

      await g.capture("gui-waveform-before-pulse");
      const pulse = await point("button", "PULSE");
      await g.page.mouse.click(pulse.clientX, pulse.clientY);
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-command")?.textContent === "Pulse sent",
      );
      const pulsing = await waitForWaveform((current) => {
        const controller = wavePulse(current);
        return controller.state === "playing" && controller.time > 0.32;
      });
      const activeWavePulse = wavePulse(pulsing);
      assert.equal(activeWavePulse.state, "playing");
      assert.equal(
        scan(pulsing).state,
        "paused",
        "PULSE resumed disabled SCAN",
      );
      // Freeze an observed playing pose before software frame capture can outlast the burst.
      await g.call(
        "controlGalleryAnimation",
        activeWavePulse.id,
        { action: "pause" },
        true,
      );
      // Center the pulse so both baseline spans can be checked in the completed frame.
      await g.call(
        "controlGalleryAnimation",
        activeWavePulse.id,
        { action: "seek", time: 0.6 },
        true,
      );
      const pulseFrame = await g.capture("gui-waveform-pulse-active");
      const capturedWaveform = await waveform();
      const capturedWavePulse = wavePulse(capturedWaveform);
      assert.ok(
        !pulseFrame.inspection.entities.some(
          ({ metadata }) => metadata.symbolicId === "gui-projector-pulse",
        ),
        "PULSE must not create a 3D pulse sphere",
      );
      assert.equal(capturedWavePulse.state, "paused");
      assert.ok(
        capturedWavePulse.time > 0.32 && capturedWavePulse.time < 1.05,
        `pulse left its visible interval before capture: ${capturedWavePulse.time}`,
      );
      await recordWaveform("pulse", {
        started: activeWavePulse,
        controller: capturedWavePulse,
        scan: scan(capturedWaveform),
        opacity: capturedWaveform.pulse.opacity,
      });
      assert.ok(
        (
          await waveformDifference(
            "gui-waveform-before-pulse",
            "gui-waveform-pulse-active",
          )
        ).changedPixels > 25,
        "PULSE did not paint a visible curve burst while SCAN was off",
      );
      const baselineSignal = await sampleWaveformBaseline(
        "gui-waveform-pulse-active",
      );
      assert.ok(
        baselineSignal.every((green) => green > 150),
        `manual pulse must join a visible baseline on both sides: ${JSON.stringify(baselineSignal)}`,
      );
      assert.ok(
        Math.abs(capturedWaveform.scan.opacity - 0.35) < 1e-6,
        "PULSE must preserve the visible paused sine trace",
      );
      const pulseToggle = await point("checkbox");
      await g.page.mouse.click(pulseToggle.clientX, pulseToggle.clientY);
      await waitForWaveform(
        (current) =>
          scan(current).state === "playing" && scan(current).time > 0.2,
      );
      // Running SCAN disables UPLINK again; its skin transition animates the
      // background part's ordinary properties into the disabled sample.
      current = await waitForGui(
        (state) => !control(state, { role: "button", name: "UPLINK" }).enabled,
      );
      const redisabledRegions = await settledUplink(
        "gui-detail-uplink-redisabled",
      );
      await recordRegions("uplink-redisabled", redisabledRegions);
      assert.ok(
        meanDifference(redisabledRegions.fill, disabledRegions.fill) < 6,
        `re-disabled UPLINK fill ${JSON.stringify(redisabledRegions.fill.mean)} is not the disabled fill ${JSON.stringify(disabledRegions.fill.mean)}`,
      );
      await g.capture("gui-waveform-pulse-scan-toggled");
      const toggledPulse = await waveform();
      assert.ok(
        toggledPulse.scan.opacity > 0.85,
        "the moving sine must remain visible alongside PULSE",
      );
      assert.equal(scan(toggledPulse).state, "playing");
      assert.equal(wavePulse(toggledPulse).time, capturedWavePulse.time);
      assert.equal(
        toggledPulse.pulse.opacity,
        1,
        "SCAN must preserve the independent pulse trace",
      );
      assert.ok(
        (
          await waveformDifference(
            "gui-waveform-pulse-active",
            "gui-waveform-pulse-scan-toggled",
          )
        ).changedPixels > 25,
        "the sine did not visibly move while the independent pulse remained in place",
      );
      await recordWaveform("baseline", {
        baselineSignal,
        scanVisible: true,
        scanToggledWhilePulseVisible: true,
      });
      await g.call(
        "controlGalleryAnimation",
        activeWavePulse.id,
        { action: "play" },
        true,
      );
      // Await the rendered translation as well as the later controller state
      // in one panel inspection.
      const advancedPulse = await waitForWaveform(
        (current) =>
          wavePulse(current).time > capturedWavePulse.time + 0.12 &&
          current.pulse.x < capturedWaveform.pulse.x - 0.3,
      );
      assert.equal(scan(advancedPulse).state, "playing");
      assert.equal(wavePulse(advancedPulse).state, "playing");
      assert.ok(
        advancedPulse.pulse.x < capturedWaveform.pulse.x - 0.3,
        "PULSE must travel right to left alongside SCAN",
      );
      await recordWaveform("direction", {
        pulseStart: capturedWaveform.pulse.x,
        pulseAdvanced: advancedPulse.pulse.x,
        scan: scan(advancedPulse),
        pulse: wavePulse(advancedPulse),
      });
      const advancedPulseTime = wavePulse(advancedPulse).time;
      await g.page.mouse.click(pulse.clientX, pulse.clientY);
      await waitForWaveform((current) => {
        const controller = wavePulse(current);
        return (
          controller.id === activeWavePulse.id &&
          controller.state === "playing" &&
          controller.time > 0.02 &&
          controller.time < advancedPulseTime - 0.1
        );
      });
      await waitForWaveform(
        (current) => wavePulse(current).state === "completed",
      );
      await waitForWaveform((current) => current.pulse.opacity === 0);
      await g.capture("gui-waveform-pulse-finished");
      const finishedPulse = await waveform();
      assert.equal(
        scan(finishedPulse).state,
        "playing",
        "pulse completion interrupted SCAN",
      );
      assert.equal(finishedPulse.pulse.opacity, 0);
      assert.ok(
        (
          await waveformDifference(
            "gui-waveform-pulse-active",
            "gui-waveform-pulse-finished",
          )
        ).changedPixels > 25,
        "the independent pulse did not disappear after completion",
      );
      await g.page.mouse.click(pulseToggle.clientX, pulseToggle.clientY);
      await waitForWaveform((current) => scan(current).state === "paused");
      current = await waitForGui(
        (state) => control(state, { role: "button", name: "UPLINK" }).enabled,
      );
      // The second disabled -> idle transition reuses UPLINK's skin
      // transition; it must blend back to the idle lane rather than keep the
      // disabled sample, and paint the enabled fill again.
      const reenabledRegions = await settledUplink(
        "gui-detail-uplink-reenabled",
      );
      await recordRegions("uplink-reenabled", reenabledRegions);
      assert.ok(
        meanDifference(reenabledRegions.fill, enabledRegions.fill) < 6,
        `re-enabled UPLINK fill ${JSON.stringify(reenabledRegions.fill.mean)} is not the enabled fill ${JSON.stringify(enabledRegions.fill.mean)}`,
      );
      assert.ok(
        meanDifference(reenabledRegions.fill, disabledRegions.fill) > 8,
        "re-enabled UPLINK still paints its disabled fill",
      );
      const beforeReset = await waitForGui();

      await g.page.locator("#reset-camera").click();
      const resetInspection = await g.waitFor(
        (inspection) =>
          JSON.stringify(transform(inspection)) ===
          JSON.stringify(cameraBeforeControls),
      );
      await g.page.waitForFunction(
        () =>
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "ready",
      );
      assertCameraFov(resetInspection, (21 * Math.PI) / 180);
      assertRetainedControls(beforeReset, await waitForGui());
      await g.capture("gui-demo-camera-reset");

      const callsign = await point("text", "CALLSIGN");
      await g.page.mouse.click(callsign.clientX, callsign.clientY);
      await g.page.waitForFunction(
        () =>
          document.querySelectorAll("textarea").length === 1 &&
          document.activeElement === document.querySelector("textarea"),
      );
      await g.page.keyboard.press("ControlOrMeta+A");
      await g.page.keyboard.insertText("NOVA-12");
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-callsign")?.textContent === "NOVA-12",
      );
      current = await waitForGui((state) => {
        const value = controlValue(state, "text", "CALLSIGN");
        return value.kind === "text" && value.value === "NOVA-12";
      });
      assert.deepEqual(transform(await g.inspect()), cameraBeforeControls);
      const controlsFrame = await g.capture("gui-demo-controls-active");
      assert.equal(controlsFrame.frame.failedDrawCalls, 0);
      const checkboxPaint = await g.call<{ changedPixels: number }>(
        "compareViewerCaptureRegion",
        "gui-demo-overview",
        "gui-demo-controls-active",
        checkboxRegion,
      );
      assert.ok(
        checkboxPaint.changedPixels > 20,
        `checked indicator region did not visibly change: ${JSON.stringify(checkboxPaint)}`,
      );
      // SCAN is a capsule switch whose knob slides to the right end while on
      // and the left end while off. Rows through the knob, inset from the
      // capsule ends, weigh each sample by its contrast with the row median
      // (the track), so the knob ink centroid falls on the knob's side.
      const [toggleX, toggleY, toggleWidth, toggleHeight] = PANEL.scan;
      const knobInk = async (label: string) => {
        const columns = 64;
        const rows = [0.3, 0.4, 0.5, 0.6, 0.7];
        const xs = Array.from(
          { length: columns },
          (_, column) =>
            toggleX + 0.06 + ((toggleWidth - 0.12) * column) / (columns - 1),
        );
        const samples = await g.call<readonly (readonly number[])[]>(
          "sampleGalleryGuiCapture",
          label,
          rows.flatMap((row) =>
            xs.map(
              (sampleX) => [sampleX, toggleY + toggleHeight * row] as const,
            ),
          ),
        );
        let weight = 0;
        let moment = 0;
        rows.forEach((_, row) => {
          const line = samples.slice(row * columns, (row + 1) * columns);
          const median = [0, 1, 2].map((channel) => {
            const values = line
              .map((pixel) => pixel[channel]!)
              .sort((a, b) => a - b);
            return values[values.length >> 1]!;
          });
          line.forEach((pixel, column) => {
            const contrast = Math.max(
              ...median.map((value, channel) =>
                Math.abs(pixel[channel]! - value),
              ),
            );
            const ink = Math.max(0, contrast - 24);
            weight += ink;
            moment += ink * xs[column]!;
          });
        });
        assert.ok(weight > 0, `${label} shows no SCAN knob`);
        return {
          centroid: moment / weight,
          centre: toggleX + toggleWidth / 2,
          weight,
        };
      };
      const knobOn = await knobInk("gui-demo-overview");
      const knobOff = await knobInk("gui-demo-controls-active");
      await recordRegions("scan-knob", { on: knobOn, off: knobOff });
      assert.ok(
        knobOn.centroid > knobOn.centre + 0.1,
        `SCAN knob is not at the right end while on: ${JSON.stringify(knobOn)}`,
      );
      assert.ok(
        knobOff.centroid < knobOff.centre - 0.1,
        `SCAN knob is not at the left end while off: ${JSON.stringify(knobOff)}`,
      );
      // Pointer clicks take toggle focus without the focus ring: the
      // toggle's top border row after the click carries the blue track
      // border and no amber ring pixel.
      const borderRow = Array.from(
        { length: 12 },
        (_, column) =>
          [
            toggleX + toggleWidth * (0.2 + (0.6 * column) / 11),
            toggleY + 0.008,
          ] as const,
      );
      const borderSamples = await g.call<readonly (readonly number[])[]>(
        "sampleGalleryGuiCapture",
        "gui-demo-controls-active",
        borderRow,
      );
      const ringPixels = borderSamples.filter(
        ([red, , blue]) => red! > 150 && red! > blue! + 30,
      );
      assert.equal(
        ringPixels.length,
        0,
        `pointer-clicked SCAN toggle shows focus ring pixels: ${JSON.stringify(borderSamples)}`,
      );
      assert.ok(
        borderSamples.filter(
          ([, , blue], index) => blue! > borderSamples[index]![0]! + 30,
        ).length >= 6,
        `SCAN border row missed the track border: ${JSON.stringify(borderSamples)}`,
      );
      const sliderPaint = await g.call<{ changedPixels: number }>(
        "compareViewerCaptureRegion",
        "gui-demo-overview",
        "gui-demo-controls-active",
        sliderRegion,
      );
      assert.ok(
        sliderPaint.changedPixels > 80,
        `slider thumb did not visibly move: ${JSON.stringify(sliderPaint)}`,
      );
      // The gain fill stays flush with the track: it is not inset by the
      // track border. Samples just inside the track's left edge, across the
      // track band, carry the bright cyan fill for any committed value.
      // Thresholds read the rendered frame, which tone maps brighter than
      // authored values: the muted border and the dark panel stay well below
      // the green/blue floors.
      const [sliderX, sliderY, , sliderHeight] = PANEL.gain;
      const fillColumn = [-0.01, 0, 0.01].map(
        (dy) => [sliderX + 0.05, sliderY + sliderHeight / 2 + dy] as const,
      );
      const fillSamples = await g.call<readonly (readonly number[])[]>(
        "sampleGalleryGuiCapture",
        "gui-demo-controls-active",
        fillColumn,
      );
      for (const [, green, blue] of fillSamples) {
        assert.ok(
          green! > 180 && blue! > 200,
          `gain fill is not flush with the track start: ${JSON.stringify(fillSamples)}`,
        );
      }
      assert.ok(
        (await g.difference("gui-demo-overview", "gui-demo-controls-active"))
          .changedPixels > 500,
        "trusted control input did not produce a visible GUI demo change",
      );

      const identityBeforeSkin = current;
      const sceneBeforeSkin = await g.inspect();
      const waveformBeforeSkin = await waveform();
      const projectorBeforeSkin = {
        core: dynamicProperty(sceneBeforeSkin, "gui-projector-core", "accent"),
        light: fieldsWith(sceneBeforeSkin, "gui-projector-light", "intensity"),
        scanController: scan(waveformBeforeSkin).id,
        wavePulseController: wavePulse(waveformBeforeSkin).id,
      };
      const callsignBeforeSkin = control(identityBeforeSkin, {
        role: "text",
        name: "CALLSIGN",
      });
      assert.equal(callsignBeforeSkin.focused, true);
      assert.deepEqual(
        identityBeforeSkin.controls.filter(({ focused }) => focused).length,
        1,
      );
      const stillFocused = (state: GalleryGuiState) => {
        const focused = state.controls.filter(({ focused }) => focused);
        assert.equal(focused.length, 1, "reskin moved or cleared focus");
        assert.equal(
          focused[0]!.target.entity,
          callsignBeforeSkin.target.entity,
        );
      };
      // Exercise the production machine-client action while the editor remains
      // focused, which isolates reskinning from normal pointer focus transfer.
      const ember = await g.call<GalleryGuiState>(
        "galleryGuiAction",
        { role: "button", name: "EMBER" },
        { kind: "press" },
      );
      await g.page.waitForFunction(
        () => document.querySelector("#gui-skin")?.textContent === "ember",
      );
      const emberState = await waitForGui();
      assertRetainedControls(identityBeforeSkin, emberState, false);
      stillFocused(ember);
      stillFocused(emberState);
      const emberFrame = await g.capture("gui-demo-ember-active");
      const emberCore = dynamicProperty(
        emberFrame.inspection,
        "gui-projector-core",
        "accent",
      );
      const emberLight = fieldsWith(
        emberFrame.inspection,
        "gui-projector-light",
        "intensity",
      );
      assert.notDeepEqual(
        emberCore,
        projectorBeforeSkin.core,
        "Ember did not recolor the projector cube",
      );
      assert.notDeepEqual(
        [emberLight.r, emberLight.g, emberLight.b],
        [
          projectorBeforeSkin.light.r,
          projectorBeforeSkin.light.g,
          projectorBeforeSkin.light.b,
        ],
        "Ember did not recolor the projector light",
      );
      const emberWaveform = await waveform();
      assert.equal(
        scan(emberWaveform).id,
        projectorBeforeSkin.scanController,
        "reskin replaced the waveform scan controller",
      );
      assert.equal(
        wavePulse(emberWaveform).id,
        projectorBeforeSkin.wavePulseController,
        "reskin replaced the waveform pulse controller",
      );
      assert.equal(
        animationFor(emberFrame.inspection, "gui-projector-beam").id,
        dustController.id,
        "reskin replaced the ambient dust controller",
      );
      const skinDifference = await g.difference(
        "gui-demo-controls-active",
        "gui-demo-ember-active",
      );
      assert.ok(skinDifference.changedPixels > 1_000);
      assert.ok(skinDifference.meanAbsoluteChannelDifference > 1);
      assert.equal(emberFrame.frame.failedDrawCalls, 0);
      assert.deepEqual(emberFrame.inspection.renderDiagnostics, []);

      // Neon reskins the same controls through shape-material lanes,
      // preserving control identity and focus throughout.
      const neon = await g.call<GalleryGuiState>(
        "galleryGuiAction",
        { role: "button", name: "NEON" },
        { kind: "press" },
      );
      await g.page.waitForFunction(
        () => document.querySelector("#gui-skin")?.textContent === "neon",
      );
      const neonState = await waitForGui();
      assertRetainedControls(identityBeforeSkin, neonState, false);
      stillFocused(neon);
      stillFocused(neonState);
      const neonFrame = await g.capture("gui-demo-neon-active");
      const neonCore = dynamicProperty(
        neonFrame.inspection,
        "gui-projector-core",
        "accent",
      );
      assert.notDeepEqual(
        neonCore,
        emberCore,
        "Neon did not recolor the projector cube",
      );
      const neonWaveform = await waveform();
      assert.equal(
        scan(neonWaveform).id,
        projectorBeforeSkin.scanController,
        "reskin replaced the waveform scan controller",
      );
      assert.equal(
        wavePulse(neonWaveform).id,
        projectorBeforeSkin.wavePulseController,
        "reskin replaced the waveform pulse controller",
      );
      const neonDifference = await g.difference(
        "gui-demo-ember-active",
        "gui-demo-neon-active",
      );
      assert.ok(neonDifference.changedPixels > 1_000);
      assert.ok(neonDifference.meanAbsoluteChannelDifference > 1);
      assert.equal(neonFrame.frame.failedDrawCalls, 0);
      assert.deepEqual(neonFrame.inspection.renderDiagnostics, []);

      // Detailed Neon regions. Every rectangle derives from the panel layout
      // restated in the panel model.
      await withDetailView(async () => {
        const detail = await waitForGui();
        await g.capture("gui-detail-neon");
        const evidence: Record<string, unknown> = {};

        const [px, py, , ph] = PANEL.pulse;
        // SPAN, UPLINK and PURGE labels sit centred in their buttons. Button
        // labels lay out left-aligned, so the specimen tunes each button's
        // left padding and the check weighs each sample by its contrast with
        // the row median (the button fill), comparing the ink centroid to
        // the centre.
        for (const name of ["SPAN", "UPLINK", "PURGE"] as const) {
          const [bx, by, bw, bh] = controlRect({ role: "button", name });
          const columns = 48;
          const labelXs = Array.from(
            { length: columns },
            (_, column) => bx + 0.05 + ((bw - 0.1) * column) / (columns - 1),
          );
          const labelSamples = await g.call<readonly (readonly number[])[]>(
            "sampleGalleryGuiCapture",
            "gui-detail-neon",
            [0.4, 0.5, 0.6].flatMap((row) =>
              labelXs.map((sampleX) => [sampleX, by + bh * row] as const),
            ),
          );
          let weight = 0;
          let moment = 0;
          for (const [row, pixel] of labelSamples.entries()) {
            const line = Math.floor(row / columns);
            const median = [0, 1, 2].map((channel) => {
              const values = labelSamples
                .slice(line * columns, (line + 1) * columns)
                .map((sample) => sample[channel]!)
                .sort((a, b) => a - b);
              return values[values.length >> 1]!;
            });
            const contrast = Math.max(
              ...median.map((value, channel) =>
                Math.abs(pixel[channel]! - value),
              ),
            );
            const ink = Math.max(0, contrast - 24);
            weight += ink;
            moment += ink * labelXs[row % columns]!;
          }
          assert.ok(weight > 0, `${name} button shows no label ink`);
          const offset = Math.abs(moment / weight - (bx + bw / 2));
          assert.ok(offset < 0.015, `${name} label is off-centre by ${offset}`);
        }
        const iconRects = Object.fromEntries(
          Object.entries(ICON_BOXES).map(([name, box]) => [name, logical(box)]),
        ) as Record<keyof typeof ICON_BOXES, LogicalRect>;
        // Painted ink, not just the layout box, is centred on each icon's
        // intrinsic box in its documented cell: an icon box sized apart
        // from its glyph, or a cell laid out elsewhere, leaves the ink
        // off-centre.
        const inkBounds = await g.call<Record<string, LogicalRect>>(
          "galleryGuiInkBounds",
          "gui-detail-neon",
          Object.fromEntries(
            Object.entries(iconRects).map(([name, [x0, y0, x1, y1]]) => [
              name,
              [x0 - 0.03, y0 - 0.03, x1 + 0.03, y1 + 0.03] as LogicalRect,
            ]),
          ),
        );
        evidence.iconInk = inkBounds;
        for (const [name, [x0, y0, x1, y1]] of Object.entries(iconRects)) {
          const ink = inkBounds[name]!;
          for (const [axis, inkCentre, boxCentre] of [
            ["x", (ink[0] + ink[2]) / 2, (x0 + x1) / 2],
            ["y", (ink[1] + ink[3]) / 2, (y0 + y1) / 2],
          ] as const)
            assert.ok(
              Math.abs(inkCentre - boxCentre) < 0.012,
              `${name} icon ink ${axis} centre ${inkCentre} is not ${boxCentre}: ${JSON.stringify(ink)}`,
            );
        }
        // Every glyph box stays inside its cell: left-aligned dashboard,
        // right-aligned signal and leading skin icons before their labels.
        assert.ok(iconRects.dashboard[2] <= logical(PANEL.title)[0]);
        assert.ok(iconRects.pulse[2] <= px + 1.08);
        for (const skin of ["aurora", "ember", "neon"] as const)
          assert.ok(iconRects[skin][2] <= PANEL.skin(skin)[0] + 0.27);
        const [ix, iy, iw, ih] = PANEL.callsign;
        // PULSE bands above/below its centre row, then farther out.
        const band = (x0: number, top: boolean): LogicalRect => [
          px + x0,
          top ? py + 0.04 : py + ph - 0.12,
          px + x0 + 0.3,
          top ? py + 0.12 : py + ph - 0.04,
        ];
        const neon = await regionStats<string>("gui-detail-neon", {
          ...iconRects,
          nearTop: band(0.3, true),
          nearBottom: band(0.3, false),
          middle: band(1.2, false),
          farTop: band(2.6, true),
          farBottom: band(2.6, false),
          halo: [px - 0.06, py + 0.25, px - 0.02, py + ph - 0.25],
          haloBaseline: [px - 0.2, py + 0.25, px - 0.16, py + ph - 0.25],
          ringTop: [ix + 0.35 * iw, iy + 0.004, ix + 0.65 * iw, iy + 0.02],
          ringBottom: [
            ix + 0.35 * iw,
            iy + ih - 0.02,
            ix + 0.65 * iw,
            iy + ih - 0.004,
          ],
          ringRight: [
            ix + iw - 0.02,
            iy + 0.3 * ih,
            ix + iw - 0.004,
            iy + 0.7 * ih,
          ],
          hollow: [ix + 0.62 * iw, iy + 0.3 * ih, ix + 0.9 * iw, iy + 0.7 * ih],
        });
        evidence.regions = neon;
        for (const name of Object.keys(ICON_CODE_POINTS)) {
          const stats = neon[name]!;
          assert.ok(
            stats.max[1] > 170 && stats.max[2] > 170,
            `${name} icon cell did not paint its glyph: ${JSON.stringify(stats)}`,
          );
        }

        // PULSE fill falls off radially from its icon cell: symmetric above
        // and below the centre, decreasing outward, flat beyond the radius.
        const { nearTop, nearBottom, middle, farTop, farBottom } = neon;
        assert.ok(
          Math.abs(nearTop!.mean[1] - nearBottom!.mean[1]) < 10 &&
            nearBottom!.mean[1] > middle!.mean[1] + 12 &&
            middle!.mean[1] > farBottom!.mean[1] + 12 &&
            Math.abs(farTop!.mean[1] - farBottom!.mean[1]) < 8,
          "PULSE lost its radial gradient",
        );

        // Neon's resting PULSE halo brightens only the band beside the
        // button.
        assert.ok(
          neon.halo!.mean[1] > neon.haloBaseline!.mean[1] + 20 &&
            neon.halo!.mean[2] > neon.haloBaseline!.mean[2] + 20,
          "PULSE halo is missing",
        );

        // The focused callsign ring strokes the edge and leaves the centre clear.
        stillFocused(detail);
        assert.ok(
          [neon.ringTop!, neon.ringBottom!, neon.ringRight!].every(
            ({ mean: [r, , b] }) => r > 150 && r > b + 30,
          ),
          "focused callsign ring lost its Neon focus colour",
        );
        assert.ok(
          neon.hollow!.mean[0] < 60 &&
            neon.hollow!.mean[2] > neon.hollow!.mean[0],
          "focus ring filled the callsign centre",
        );

        // SPAN widens its frame; corner radii keep their authored units, so
        // every corner and edge region is unchanged, and the flex spacer
        // after the frame absorbs the resize, so the SPAN button that
        // follows it in tree order paints in place.
        const frameRects = ([x, y, width, height]: ContentRect) => ({
          topLeft: [x - 0.03, y - 0.03, x + 0.15, y + 0.15] as LogicalRect,
          bottomLeft: [
            x - 0.03,
            y + height - 0.15,
            x + 0.15,
            y + height + 0.03,
          ] as LogicalRect,
          left: [
            x - 0.03,
            y + 0.12,
            x + 0.06,
            y + height - 0.12,
          ] as LogicalRect,
          topRight: [
            x + width - 0.15,
            y - 0.03,
            x + width + 0.03,
            y + 0.15,
          ] as LogicalRect,
          bottomRight: [
            x + width - 0.15,
            y + height - 0.15,
            x + width + 0.03,
            y + height + 0.03,
          ] as LogicalRect,
          top: [
            x + width / 2 - 0.1,
            y - 0.03,
            x + width / 2 + 0.1,
            y + 0.06,
          ] as LogicalRect,
          button: logical(PANEL.span),
        });
        const narrow = PANEL.spanFrame(false);
        // Beyond the narrow frame but inside the wide one, clear of its text.
        const widened: LogicalRect = [
          narrow[0] + 1.4,
          narrow[1] + 0.12,
          narrow[0] + 1.7,
          narrow[1] + 0.26,
        ];
        const frameEntity = async () =>
          sceneEntity(await panelInspection(), "gui-span-frame/fill").id;
        const narrowFrame = await frameEntity();
        const narrowRegions = await regionStats("gui-detail-neon", {
          ...frameRects(narrow),
          widened,
        });
        await g.call(
          "galleryGuiAction",
          { role: "button", name: "SPAN" },
          { kind: "press" },
        );
        await g.page.waitForFunction(
          () => document.querySelector("#gui-span")?.textContent === "wide",
        );
        const wide = PANEL.spanFrame(true);
        await waitForPanel(
          (inspection) =>
            Math.abs(
              Number(
                fieldsWith(inspection, "gui-span-frame", "min_height").width,
              ) - wide[2],
            ) < 1e-5,
        );
        assert.equal(
          await frameEntity(),
          narrowFrame,
          "SPAN replaced its frame instead of resizing it",
        );
        await g.capture("gui-detail-span-wide");
        const wideRegions = await regionStats("gui-detail-span-wide", {
          ...frameRects(wide),
          button: logical(PANEL.span),
          widened,
        });
        evidence.span = { narrow, wide, narrowRegions, wideRegions };
        assert.ok(
          meanDifference(wideRegions.widened, narrowRegions.widened) > 20,
          "SPAN did not paint its widened frame",
        );
        for (const key of Object.keys(frameRects(narrow)) as (keyof ReturnType<
          typeof frameRects
        >)[]) {
          const difference = meanDifference(
            narrowRegions[key],
            wideRegions[key],
          );
          assert.ok(
            difference < 4,
            `SPAN ${key} region changed by ${difference} when resized`,
          );
        }
        await g.call(
          "galleryGuiAction",
          { role: "button", name: "SPAN" },
          { kind: "press" },
        );
        await g.page.waitForFunction(
          () => document.querySelector("#gui-span")?.textContent === "narrow",
        );
        await recordRegions("neon", evidence);
      });

      await g.page.locator("#ipp-world-canvas").scrollIntoViewIfNeeded();
      // Wheel over the telemetry readouts, above the nested event log.
      const [tx, ty, tw] = PANEL.telemetry;
      const [scroll] = await projectContent(g, [[tx + tw * 0.5, ty + 0.15]]);
      const cameraBeforeScroll = transform(await g.inspect());
      await g.capture("gui-demo-before-scroll");
      await g.page.mouse.move(scroll!.clientX, scroll!.clientY);
      // One wheel step scrolls the telemetry view by an eighth of its 0.78
      // viewport.
      const beforeNotch = telemetryScrollViews(await waitForGui()).outer;
      const notchTarget = Math.min(
        beforeNotch.offset[1] + WHEEL_STEP,
        beforeNotch.capacity[1],
      );
      assert.ok(
        notchTarget > beforeNotch.offset[1] + 0.05,
        `telemetry has no room for a wheel notch: ${JSON.stringify(beforeNotch.capacity)}`,
      );
      await g.page.mouse.wheel(0, WHEEL_NOTCH_PIXELS);
      const afterNotch = telemetryScrollViews(
        await waitForGui(
          (state) =>
            Math.abs(
              telemetryScrollViews(state).outer.offset[1] - notchTarget,
            ) < 1e-4,
        ),
      ).outer;
      await scenario.evidence.record("telemetry-wheel-notch", {
        before: { offset: beforeNotch.offset, capacity: beforeNotch.capacity },
        after: { offset: afterNotch.offset, capacity: afterNotch.capacity },
        viewport: afterNotch.viewport,
      });
      await g.page.mouse.wheel(0, 5 * WHEEL_NOTCH_PIXELS);
      await g.settle();
      assert.deepEqual(
        transform(await g.inspect()),
        cameraBeforeScroll,
        "GUI scroll was also processed as a camera gesture",
      );
      await g.capture("gui-demo-after-scroll");
      assert.ok(
        (await g.difference("gui-demo-before-scroll", "gui-demo-after-scroll"))
          .changedPixels > 100,
        "event stream did not visibly scroll",
      );

      const auroraPoint = await point("button", "AURORA");
      await g.page.mouse.click(auroraPoint.clientX, auroraPoint.clientY);
      await g.page.waitForFunction(
        () => document.querySelector("#gui-skin")?.textContent === "aurora",
      );
      const emberPoint = await point("button", "EMBER");
      await g.page.mouse.click(emberPoint.clientX, emberPoint.clientY);
      await g.page.waitForFunction(
        () => document.querySelector("#gui-skin")?.textContent === "ember",
      );
      assertRetainedControls(identityBeforeSkin, await waitForGui(), false);
      await g.capture("gui-demo-final-ember-controls");

      const mountedEntity = guiEntity(await g.inspect())!;
      const mountedPanel = await waitForGui();
      const mountedWaveform = await waveform();
      const mountedScan = scan(mountedWaveform);
      const mountedWavePulse = wavePulse(mountedWaveform);
      const fullSceneFrame = await g.capture("gui-demo-full-before-isolation");
      const vectorButton = g.page.locator("#gui-vector-only");
      assert.equal(await vectorButton.getAttribute("aria-pressed"), "false");
      await vectorButton.click();
      const isolated = await g.waitFor(
        (inspection) =>
          !inspection.entities.some(({ metadata }) =>
            metadata.symbolicId?.startsWith("gui-projector-"),
          ),
      );
      assert.equal(await vectorButton.getAttribute("aria-pressed"), "true");
      assert.deepEqual(
        isolated.entities.map(({ metadata }) => metadata.symbolicId).sort(),
        ["gallery-camera", PANEL_ENTITY],
      );
      assert.ok(
        !isolated.controllers?.some(({ id }) => id === dustController.id),
        "vector isolation retained the projector dust controller",
      );
      assertRetainedControls(mountedPanel, await waitForGui());
      const vectorFrame = await g.capture("gui-demo-vector-only");
      assert.ok(
        vectorFrame.frame.drawCalls > 0,
        "vector panel stopped drawing",
      );
      assert.ok(
        vectorFrame.frame.drawCalls < fullSceneFrame.frame.drawCalls,
        "removing the projector did not reduce draw calls",
      );
      assert.ok(
        (
          await g.difference(
            "gui-demo-full-before-isolation",
            "gui-demo-vector-only",
          )
        ).changedPixels > 100,
        "vector isolation did not visibly remove the projector",
      );
      await vectorButton.click();
      await g.waitFor((inspection) =>
        inspection.entities.some(
          ({ metadata }) => metadata.symbolicId === "gui-projector-beam",
        ),
      );
      assert.equal(await vectorButton.getAttribute("aria-pressed"), "false");
      const restoredFrame = await g.capture("gui-demo-projector-restored");
      assert.ok(
        restoredFrame.frame.drawCalls > vectorFrame.frame.drawCalls,
        "restoring the projector did not restore its draws",
      );
      await g.navigate("shapes");
      const cleaned = await g.waitFor(
        (inspection) =>
          guiEntity(inspection) === undefined &&
          [...surfaceSources].every(
            (source) =>
              !inspection.resources.some(
                (resource) => resource.source === source,
              ),
          ),
      );
      assertCameraFov(cleaned, Math.PI / 4);
      // Removing the panel destroys its creator-owned World, with the
      // waveform controllers it held.
      await waitForPanelWorldGone(
        "navigation retained the panel World and its waveform controllers",
      );
      assert.ok(
        !cleaned.controllers?.some(({ id }) => id === dustController.id),
        "navigation retained the ambient dust controller",
      );
      // IppCanvas owns its input context, and with it one hidden native text
      // buffer, for the canvas lifetime. Leaving the GUI page leaves that one
      // buffer blurred and empty, not holding the removed CALLSIGN edit.
      assert.deepEqual(
        await g.page.evaluate(() => {
          const buffers = [...document.querySelectorAll("textarea")];
          return {
            count: buffers.length,
            focused: buffers.some(
              (buffer) => buffer === document.activeElement,
            ),
            values: buffers.map((buffer) => buffer.value),
          };
        }),
        { count: 1, focused: false, values: [""] },
        "navigation left the native text buffer holding the removed panel's text",
      );
      assert.deepEqual(
        (await g.call<{ session: bigint }>("observeViewer")).session,
        baseline.session,
        "authored gallery navigation replaced the shared session",
      );

      await g.navigate("gui");
      const returned = await waitForGui();
      assert.equal(
        await g.page.locator("#gui-vector-only").getAttribute("aria-pressed"),
        "false",
      );
      assert.notEqual(guiEntity(await g.inspect())!.id, mountedEntity.id);
      assert.notDeepEqual(
        returned.world,
        mountedPanel.world,
        "reentry reused the removed panel World",
      );
      const returnedInspection = await g.inspect();
      assertCameraFov(returnedInspection, (21 * Math.PI) / 180);
      const returnedWaveform = await waveform();
      const returnedScanner = scan(returnedWaveform);
      assertNoScanner(returnedInspection);
      assert.equal(returnedScanner.state, "playing");
      const returnedDust = animationFor(
        returnedInspection,
        "gui-projector-beam",
      );
      assert.notEqual(returnedDust.id, dustController.id);
      assert.equal(returnedDust.state, "playing");
      assert.deepEqual(controlValue(returned, "checkbox"), {
        kind: "bool",
        value: true,
      });
      assertScalarValue(returned, 0.64);
      assert.deepEqual(controlValue(returned, "text", "CALLSIGN"), {
        kind: "text",
        value: "VESPER-7",
      });
      await recordWaveform("lifecycle", {
        removedWorld: mountedPanel.world,
        removedScan: mountedScan.id,
        removedPulse: mountedWavePulse.id,
        returnedWorld: returned.world,
        returnedScan: returnedScanner.id,
        returnedPulse: wavePulse(returnedWaveform).id,
      });
      await g.capture("gui-demo-returned");
      assert.deepEqual(g.errors, []);
    },
  );
});

/** The gallery panel's cache policy, restated independently of scene.tsx. */
const CACHE_DIRECT_DISTANCE = 20;
const CACHE_TEXELS_PER_METRE = 80;
const CACHE_REFRESH_HZ = 15;
const CACHE_HYSTERESIS = 0.1;

/**
 * Cache image size in a band: content metres times the band's halved density,
 * rounded up. Surface sizes are f32 fields, so 7.4 m reads as 7.40000010 m;
 * like the renderer, a 1e-4 texel tolerance keeps that error from adding a texel.
 */
function expectedCacheSize(band: number): readonly [number, number] {
  const density = CACHE_TEXELS_PER_METRE / 2 ** (band - 1);
  return [
    Math.ceil(Math.fround(7.4) * density - 1e-4),
    Math.ceil(Math.fround(4.8) * density - 1e-4),
  ];
}

/**
 * Cached versus direct panel tolerance, taken from the retained-gui text
 * comparison: at most 3.5% of panel pixels may differ by more than 64 levels
 * and bright text/glow masks must overlap by at least 0.85. The band-one
 * image resamples the panel once more than direct drawing, which moves glyph
 * and border edges by about one texel.
 */
const CACHE_COMPARISON = {
  channelThreshold: 64,
  maxChangedFraction: 0.035,
  minTextAgreement: 0.85,
} as const;

interface CacheObservation {
  readonly label: string;
  readonly record: SurfaceCacheRecord | undefined;
  readonly statistics: RenderStatisticsSnapshot | undefined;
  readonly repaints: number;
  readonly allocations: number;
  readonly reuses: number;
  readonly direct: number;
}

function cacheDelta(before: CacheObservation, after: CacheObservation) {
  return {
    repaints: after.repaints - before.repaints,
    allocations: after.allocations - before.allocations,
    reuses: after.reuses - before.reuses,
  };
}

function decodeRegion(region: {
  width: number;
  height: number;
  pixels: string;
}): RgbaFrame {
  return {
    width: region.width,
    height: region.height,
    pixels: new Uint8Array(Buffer.from(region.pixels, "base64")),
  };
}

/** Bright text, icon and glow pixels of the panel. */
function isBright(r: number, g: number, b: number): boolean {
  return Math.max(r, g, b) >= 170;
}

test("Gallery GUI panel caches distant presentation within direct-rendering tolerance", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "GUI demo Surface cache",
    {
      ...galleryEnvironment,
      evidenceParent: resolve(
        "target/integration-artifacts/gallery-gui/surface-cache",
      ),
    },
    context.signal,
    async (scenario) => {
      const started = performance.now();
      const g = await openGallery(scenario, { initialPage: "gui" });
      await g.page.waitForFunction(
        () =>
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "ready",
      );
      // Keep the pointer off the canvas: hover would promote the panel.
      await g.page.mouse.move(1, 1);
      // Compare the vector panel alone, as the gallery's isolation control
      // intends, so projector geometry never occludes a placed panel.
      await g.page.locator("#gui-vector-only").click();
      await g.waitFor(
        (inspection) =>
          !inspection.entities.some(({ metadata }) =>
            metadata.symbolicId?.startsWith("gui-projector-"),
          ),
      );
      await g.page.mouse.move(1, 1);
      const evidence: Record<string, unknown> = {};
      const record = async (name: string, value: unknown) => {
        evidence[name] = value;
        await writeFile(
          join(scenario.evidence.directory, "surface-cache-evidence.json"),
          JSON.stringify(
            evidence,
            (_key, value) =>
              typeof value === "bigint" ? String(value) : value,
            2,
          ) + "\n",
        );
      };
      const observe = async (label: string): Promise<CacheObservation> => {
        const entity = await g.call<bigint | undefined>(
          "galleryEntityId",
          PANEL_ENTITY,
        );
        const { frame } = await g.capture(label);
        const statistics = frame.statistics;
        const surfaces = statistics?.surfaces;
        assert.ok(surfaces, "Surface cache diagnostics are unavailable");
        const records = surfaces.surfaceCaches;
        const observation = {
          label,
          record:
            entity === undefined
              ? undefined
              : records.find((entry) => entry.entity === entity),
          statistics,
          repaints: surfaces.totalSurfaceCacheRepaints,
          allocations: surfaces.totalSurfaceCacheAllocations,
          reuses: surfaces.totalSurfaceCacheReuses,
          direct: surfaces.totalSurfaceCacheDirect,
        };
        const { ingress: _ingress, ...stats } = statistics!;
        await record(label, { ...stats, records });
        return observation;
      };
      // Poll completed frames until the panel's record satisfies `ready`.
      const observeUntil = async (
        label: string,
        ready: (record: SurfaceCacheRecord | undefined) => boolean,
      ) => {
        const deadline = performance.now() + 10_000;
        for (;;) {
          const observation = await observe(label);
          if (ready(observation.record)) return observation;
          assert.ok(
            performance.now() < deadline,
            `${label}: cache record stayed ${JSON.stringify(observation.record, (_key, value) => (typeof value === "bigint" ? String(value) : value))}`,
          );
        }
      };
      const reused = (band: number) => (entry?: SurfaceCacheRecord) =>
        entry?.mode === "reused" && entry.band === band;
      const assertWarm = (observation: CacheObservation, band: number) => {
        const [width, height] = expectedCacheSize(band);
        assert.equal(observation.record?.mode, "reused");
        assert.equal(observation.record?.band, band);
        assert.equal(observation.record?.width, width);
        assert.equal(observation.record?.height, height);
        assert.equal(observation.record?.residentBytes, 4 * width * height);
        // A warm frame composites the unchanged image: no raster or upload work.
        assert.equal(observation.statistics!.surfaces!.surfaceCacheReuses, 1);
        assert.equal(observation.statistics!.surfaces!.surfaceCacheRepaints, 0);
        assert.equal(observation.statistics!.surfaces!.surfaceCacheDirect, 0);
        assert.equal(
          observation.statistics!.surfaces!.surfaceCacheAllocations,
          0,
        );
        assert.equal(observation.statistics!.frame.uploadedBytes, 0);
        assert.equal(
          observation.statistics!.surfaces!.surfaceCacheResidentBytes,
          4 * width * height,
        );
      };
      const setMode = async (mode: "automatic" | "cached" | "direct") => {
        await g.page.locator("#gui-surface-cache").selectOption(mode);
        await g.settle();
      };
      const place = (distance: number, replace = true) =>
        g.call("faceGalleryGuiToCamera", 0.92, distance, replace);
      const panelBounds = async () => {
        const corners = await g.call<readonly { x: number; y: number }[]>(
          "projectGalleryPoints",
          PANEL_ENTITY,
          [
            [-3.7, 2.4, 0],
            [3.7, 2.4, 0],
            [-3.7, -2.4, 0],
            [3.7, -2.4, 0],
          ],
        );
        return [
          Math.min(...corners.map((p) => p.x)),
          Math.min(...corners.map((p) => p.y)),
          Math.max(...corners.map((p) => p.x)),
          Math.max(...corners.map((p) => p.y)),
        ] as const;
      };
      // Cached (actual) against direct (expected) at one placement and camera.
      const compareWithDirect = async (name: string, band: number) => {
        const cached = await observeUntil(`${name}-cached`, reused(band));
        assertWarm(cached, band);
        await setMode("direct");
        const direct = await observeUntil(
          `${name}-direct`,
          (entry) => entry === undefined,
        );
        assert.equal(direct.statistics!.surfaces!.surfaceCacheEntries, 0);
        assert.equal(direct.statistics!.surfaces!.surfaceCacheResidentBytes, 0);
        assert.equal(direct.statistics!.surfaces!.surfaceCacheDirect, 0);
        const bounds = await panelBounds();
        const [expected, actual] = await Promise.all(
          [direct.label, cached.label].map((label) =>
            g
              .call<{
                width: number;
                height: number;
                pixels: string;
              }>("viewerCaptureRegionPixels", label, bounds)
              .then(decodeRegion),
          ),
        );
        const difference = compareFrames(
          expected!,
          actual!,
          CACHE_COMPARISON.channelThreshold,
        );
        const expectedText = mask(expected!, isBright);
        const actualText = mask(actual!, isBright);
        const comparison = {
          ...difference,
          width: expected!.width,
          height: expected!.height,
          directTextPixels: count(expectedText),
          cachedTextPixels: count(actualText),
          textAgreement: intersectionOverUnion(expectedText, actualText),
        };
        await Promise.all([
          writeFile(
            join(scenario.evidence.directory, `${name}-expected.png`),
            encodePng(expected!),
          ),
          writeFile(
            join(scenario.evidence.directory, `${name}-actual.png`),
            encodePng(actual!),
          ),
          writeFile(
            join(scenario.evidence.directory, `${name}-diff.png`),
            encodePng(
              differenceImage(
                expected!,
                actual!,
                CACHE_COMPARISON.channelThreshold,
              ),
            ),
          ),
          record(`${name}-comparison`, comparison),
        ]);
        assert.ok(comparison.directTextPixels > 500, "panel text is missing");
        assert.ok(
          comparison.changedFraction <= CACHE_COMPARISON.maxChangedFraction &&
            comparison.textAgreement >= CACHE_COMPARISON.minTextAgreement,
          `${name}: cached panel differs from direct presentation: ${JSON.stringify(comparison)}`,
        );
        await setMode("automatic");
        return comparison;
      };

      // Static content makes repaint counts exact: stop the scrolling trace.
      await g.call(
        "galleryGuiAction",
        { role: "checkbox" },
        { kind: "toggle" },
      );
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-autoscan")?.textContent === "standby",
      );
      await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));

      // The authored camera is inside the direct distance.
      const authored = await g.inspect();
      const camera = transform(authored);
      const panel = fieldsWith(authored, PANEL_ENTITY, "sx");
      const authoredDistance = Math.hypot(
        ...["x", "y", "z"].map(
          (axis) => Number(panel[axis]) - Number(camera[axis]),
        ),
      );
      assert.ok(authoredDistance < CACHE_DIRECT_DISTANCE);
      const near = await observeUntil(
        "surface-cache-authored-near",
        (entry) => entry?.mode === "near",
      );
      assert.equal(near.record?.band, 0);
      assert.equal(near.record?.residentBytes, 0);
      assert.equal(near.statistics!.surfaces!.surfaceCacheDirect, 1);
      assert.equal(near.statistics!.surfaces!.surfaceCacheRepaints, 0);
      assert.equal(near.statistics!.surfaces!.surfaceCacheResidentBytes, 0);
      await record("authored-distance", authoredDistance);

      // Cold band-one frame: one allocation and one repaint, then reuse.
      const beforeCold = near;
      await place(26, false);
      const warm = await observeUntil("surface-cache-band1-warm", reused(1));
      assertWarm(warm, 1);
      assert.deepEqual(
        { ...cacheDelta(beforeCold, warm), reuses: 0 },
        { repaints: 1, allocations: 1, reuses: 0 },
      );
      assert.equal(warm.record?.repaints, 1);
      const aurora = await compareWithDirect("surface-cache-aurora-band1", 1);

      // Hysteresis: 42 m stays in band one (boundary 40 m + 10%), 46 m moves
      // to band two with one resize, 38 m stays there and 34 m returns.
      const bands: Array<readonly [number, number, number]> = [
        [2 * CACHE_DIRECT_DISTANCE * (1 + CACHE_HYSTERESIS) - 2, 1, 0],
        [2 * CACHE_DIRECT_DISTANCE * (1 + CACHE_HYSTERESIS) + 2, 2, 1],
        [2 * CACHE_DIRECT_DISTANCE * (1 - CACHE_HYSTERESIS) + 2, 2, 0],
        [2 * CACHE_DIRECT_DISTANCE * (1 - CACHE_HYSTERESIS) - 2, 1, 1],
      ];
      let previous = await observeUntil("surface-cache-readmitted", reused(1));
      const sizes: Array<readonly [number, number]> = [];
      for (const [distance, band, allocations] of bands) {
        await place(distance);
        const current = await observeUntil(
          `surface-cache-${distance}m`,
          reused(band),
        );
        assertWarm(current, band);
        assert.deepEqual(
          { ...cacheDelta(previous, current), reuses: 0 },
          { repaints: allocations, allocations, reuses: 0 },
          `${distance} m`,
        );
        sizes.push([current.record!.width, current.record!.height]);
        previous = current;
      }
      assert.ok(sizes[1]![0] < sizes[0]![0] && sizes[1]![1] < sizes[0]![1]);

      // A viewport resize keeps the fixed texel density: no repaint.
      await place(26);
      previous = await observeUntil("surface-cache-before-resize", reused(1));
      const viewport = g.page.viewportSize()!;
      await g.page.setViewportSize({
        width: viewport.width - 160,
        height: viewport.height - 80,
      });
      try {
        const resized = await observeUntil(
          "surface-cache-viewport-resized",
          reused(1),
        );
        assertWarm(resized, 1);
        assert.deepEqual(cacheDelta(previous, resized).repaints, 0);
        assert.deepEqual(cacheDelta(previous, resized).allocations, 0);
      } finally {
        await g.page.setViewportSize(viewport);
      }
      previous = await observeUntil(
        "surface-cache-viewport-restored",
        reused(1),
      );

      // A skin switch is paint, not a resource change: it repaints at the
      // band's cadence and settles on the latest state.
      await g.call(
        "galleryGuiAction",
        { role: "button", name: "EMBER" },
        { kind: "press" },
      );
      await g.page.waitForFunction(
        () => document.querySelector("#gui-skin")?.textContent === "ember",
      );
      await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));
      const reskinned = await observeUntil(
        "surface-cache-ember-settled",
        reused(1),
      );
      const reskin = cacheDelta(previous, reskinned);
      const reskinSeconds =
        (reskinned.record!.paintedAtMs - previous.record!.paintedAtMs) / 1000;
      assert.equal(reskin.allocations, 0);
      assert.ok(reskin.repaints >= 1, "the skin switch never repainted");
      assert.ok(
        reskin.repaints <= Math.ceil(reskinSeconds * CACHE_REFRESH_HZ) + 1,
        `skin repaints exceeded the ${CACHE_REFRESH_HZ} Hz cap: ${JSON.stringify({ reskin, reskinSeconds })}`,
      );
      await record("ember-repaints", { ...reskin, reskinSeconds });
      const ember = await compareWithDirect("surface-cache-ember-band1", 1);

      // Continuous scanning keeps the trace current without exceeding the
      // cap: the cached image repaints at most at the cap, and either keeps
      // repainting or the renderer presents the changing panel directly.
      previous = await observeUntil("surface-cache-before-scan", reused(1));
      await g.call(
        "galleryGuiAction",
        { role: "checkbox" },
        { kind: "toggle" },
      );
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-autoscan")?.textContent === "enabled",
      );
      const scanStart = await observe("surface-cache-scan-start");
      await new Promise((resolve) => setTimeout(resolve, 1_000));
      const scanEnd = await observe("surface-cache-scan-end");
      const scan = {
        ...cacheDelta(scanStart, scanEnd),
        direct: scanEnd.direct - scanStart.direct,
      };
      const scanSeconds =
        (scanEnd.record!.paintedAtMs - scanStart.record!.paintedAtMs) / 1000;
      assert.ok(
        scan.repaints >= 2 || scan.direct > 0,
        `continuous scanning starved presentation: ${JSON.stringify(scan)}`,
      );
      assert.ok(
        scan.repaints <= Math.ceil(scanSeconds * CACHE_REFRESH_HZ) + 1,
        `scan repaints exceeded the ${CACHE_REFRESH_HZ} Hz cap: ${JSON.stringify({ scan, scanSeconds })}`,
      );
      if (scan.direct === 0) assert.equal(scan.allocations, 0);
      await record("scan-repaints", { ...scan, scanSeconds });
      await g.call(
        "galleryGuiAction",
        { role: "checkbox" },
        { kind: "toggle" },
      );
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-autoscan")?.textContent === "standby",
      );
      await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));
      previous = await observeUntil("surface-cache-scan-stopped", reused(1));

      // Hover promotes the distant panel to direct presentation at once.
      const pulse = await controlPoint(g, { role: "button", name: "PULSE" });
      await g.page.mouse.move(pulse.clientX, pulse.clientY);
      const hovered = await observeUntil(
        "surface-cache-hover",
        (entry) => entry?.mode === "interaction",
      );
      assert.equal(hovered.statistics!.surfaces!.surfaceCacheDirect, 1);
      assert.equal(hovered.statistics!.surfaces!.surfaceCacheReuses, 0);
      assert.equal(hovered.statistics!.surfaces!.surfaceCacheRepaints, 0);
      const heldHover = await observe("surface-cache-hover-held");
      assert.equal(heldHover.record?.mode, "interaction");
      assert.equal(cacheDelta(hovered, heldHover).repaints, 0);
      // Leaving repaints before the changed image can be shown again. The
      // canvas corner shows only background, so the GUI observes the exit.
      const canvas = (await g.page.locator("#ipp-world-canvas").boundingBox())!;
      await g.page.mouse.move(canvas.x + 8, canvas.y + 8);
      const left = await observeUntil("surface-cache-hover-left", reused(1));
      assert.ok(cacheDelta(heldHover, left).repaints >= 1);
      assert.ok(left.record!.paintedAtMs > previous.record!.paintedAtMs);

      // Removing the GUI World content releases its image.
      await g.call("releaseGalleryGuiTransform");
      await g.navigate("shapes");
      const released = await observeUntil("surface-cache-released", () => true);
      assert.deepEqual(released.statistics!.surfaces!.surfaceCaches, []);
      assert.equal(released.statistics!.surfaces!.surfaceCacheEntries, 0);
      assert.equal(released.statistics!.surfaces!.surfaceCacheResidentBytes, 0);
      await record("summary", {
        aurora,
        ember,
        durationMs: performance.now() - started,
      });
      assert.deepEqual(g.errors, []);
    },
  );
});

type Gallery = Awaited<ReturnType<typeof openGallery>>;

/** Runtime scroll bar thickness: a twentieth of the viewport's shorter side. */
const SCROLL_BAR_THICKNESS = 0.05;

/** Shortest thumb, in bar thicknesses. */
const SCROLL_THUMB_MIN = 2;

async function waitForGuiState(
  g: Gallery,
  predicate: (state: GalleryGuiState) => boolean = () => true,
): Promise<GalleryGuiState> {
  const deadline = performance.now() + 15_000;
  let lastError: unknown;
  while (performance.now() < deadline) {
    try {
      const state = await g.call<GalleryGuiState>("galleryGuiState");
      if (predicate(state)) return state;
    } catch (failure) {
      lastError = failure;
    }
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error(
    `GUI demo did not settle${lastError instanceof Error ? `: ${lastError.message}` : ""}`,
  );
}

/** Event log entries restated from the demo: a Text at least 0.28 high
 * with a 0.05 gap below it, at 0.16 type, wrapping at 2.74. */
const EVENT_MIN_HEIGHT = 0.28;
const EVENT_GAP = 0.05;
const EVENT_FONT_SIZE = 0.16;
const EVENT_TEXT_WIDTH = 2.74;

/**
 * The event log VirtualList: its semantic scroll and item state, authored
 * item properties, and the declared items in index order.
 */
function eventLog(state: GalleryGuiState) {
  const views = telemetryScrollViews(state);
  const { itemCount, itemExtent, overscan, items } = state.eventLog;
  assert.ok(
    Number.isFinite(itemCount) &&
      Number.isFinite(itemExtent) &&
      Number.isFinite(overscan),
    "the event log has no authored item properties",
  );
  return { ...views, itemCount, itemExtent, overscan, items };
}

type EventLog = ReturnType<typeof eventLog>;

/**
 * Independent event log layout: every item takes the estimate except the
 * declared ones, which measure their greedily wrapped lines (at least the
 * minimum height) plus the gap. Positions, content extent, capacity and the
 * wanted range follow from those extents, the viewport and the overscan.
 */
function expectedEventLog(log: EventLog) {
  const glyph = FONT_METRICS.advance * EVENT_FONT_SIZE;
  const line = FONT_METRICS.lineHeight * EVENT_FONT_SIZE;
  const columns = Math.floor(EVENT_TEXT_WIDTH / glyph + 1e-4);
  const estimate = log.itemExtent;
  const count = log.itemCount;
  const viewport = PANEL.eventLog[3];
  const first = log.items[0]?.index ?? 0;
  const lines = log.items.map(({ text }) => wrapColumns(text, columns));
  const extents = lines.map(
    (wrapped) => Math.max(wrapped.length * line, EVENT_MIN_HEIGHT) + EVENT_GAP,
  );
  const starts = extents.map((_, at) =>
    extents
      .slice(0, at)
      .reduce((sum, extent) => sum + extent, first * estimate),
  );
  const declaredEnd = first + extents.length;
  const excess =
    extents.reduce((sum, extent) => sum + extent, 0) -
    extents.length * estimate;
  const position = (index: number) =>
    index < first
      ? index * estimate
      : index < declaredEnd
        ? starts[index - first]!
        : index * estimate + excess;
  const extent = (index: number) =>
    index >= first && index < declaredEnd ? extents[index - first]! : estimate;
  const content = count * estimate + excess;
  const capacity = Math.max(0, content - viewport);
  // Item containing a main-axis offset; the log holds a few hundred items
  // at most, so a walk from the first is enough.
  const itemAt = (offset: number) => {
    let index = 0;
    while (index < count - 1 && offset >= position(index) + extent(index))
      index += 1;
    return index;
  };
  const wanted = (offset: number): [number, number] => {
    if (count === 0) return [0, 0];
    const firstVisible = itemAt(offset);
    const end = offset + viewport;
    let lastVisible = itemAt(end);
    if (lastVisible > firstVisible && position(lastVisible) >= end)
      lastVisible -= 1;
    return [
      Math.max(0, firstVisible - log.overscan),
      Math.min(count, lastVisible + 1 + log.overscan),
    ];
  };
  return {
    glyph,
    line,
    columns,
    lines,
    extents,
    position,
    content,
    capacity,
    wanted,
  };
}

/**
 * Expected vertical scroll bar of one scrolling view whose viewport is
 * `rect` on screen: a track along the right edge as thick as a twentieth of
 * the shorter viewport side, and a thumb whose length is the visible
 * fraction of the track, never shorter than two thicknesses, placed by
 * offset / capacity.
 */
function expectedScrollBar(
  view: ScrollViews["outer"],
  [x, y, width, height]: ContentRect,
) {
  const thickness = SCROLL_BAR_THICKNESS * Math.min(width, height);
  const capacity = view.capacity[1];
  const extent = height + capacity;
  const length = Math.min(
    height,
    Math.max(height * (height / extent), SCROLL_THUMB_MIN * thickness),
  );
  const fraction = capacity > 0 ? view.offset[1] / capacity : 0;
  const top = y + (height - length) * fraction;
  return {
    thickness,
    track: [x + width - thickness, y, x + width, y + height] as LogicalRect,
    thumb: [x + width - thickness, top, x + width, top + length] as LogicalRect,
  };
}

/** The event log's on-screen viewport for a telemetry offset. */
function eventLogViewport(outerOffset: number): ContentRect {
  const [x, y, width, height] = PANEL.eventLog;
  return [x, y - outerOffset, width, height];
}

/** Convex hull of projected points, in order around the hull. */
function convexHull(points: readonly ProjectedPoint[]): ProjectedPoint[] {
  const sorted = [...points].sort(
    (a, b) => a.clientX - b.clientX || a.clientY - b.clientY,
  );
  const cross = (o: ProjectedPoint, a: ProjectedPoint, b: ProjectedPoint) =>
    (a.clientX - o.clientX) * (b.clientY - o.clientY) -
    (a.clientY - o.clientY) * (b.clientX - o.clientX);
  const half = (ordered: readonly ProjectedPoint[]) => {
    const chain: ProjectedPoint[] = [];
    for (const point of ordered) {
      while (
        chain.length >= 2 &&
        cross(chain[chain.length - 2]!, chain[chain.length - 1]!, point) <= 0
      )
        chain.pop();
      chain.push(point);
    }
    chain.pop();
    return chain;
  };
  return [...half(sorted), ...half([...sorted].reverse())];
}

/**
 * Compare a colour classification of one completed-frame area with the
 * logical rectangles expected to hold it, and write the expected mask, the
 * actual crop, the actual mask and their difference as PNG evidence. The
 * area maps to frame pixels through its projected corners, so the detail
 * view or the authored camera both work while the panel stays planar.
 */
async function compareMask(
  g: Gallery,
  directory: string,
  name: string,
  capture: {
    readonly label: string;
    readonly width: number;
    readonly height: number;
  },
  area: LogicalRect,
  expected: readonly LogicalRect[],
  select: (pixel: readonly [number, number, number]) => boolean,
) {
  const [x0, y0, x1, y1] = area;
  const corners = await projectContent(g, [
    [x0, y0],
    [x1, y0],
    [x0, y1],
    [x1, y1],
  ]);
  const [a, b, d] = corners as [ProjectedPoint, ProjectedPoint, ProjectedPoint];
  const region = await g.call<{
    left: number;
    top: number;
    width: number;
    height: number;
    pixels: string;
  }>("viewerCaptureRegionPixels", capture.label, [
    Math.min(...corners.map(({ x }) => x)),
    Math.min(...corners.map(({ y }) => y)),
    Math.max(...corners.map(({ x }) => x)),
    Math.max(...corners.map(({ y }) => y)),
  ]);
  const crop = decodeRegion(region);
  const det = (b.x - a.x) * (d.y - a.y) - (b.y - a.y) * (d.x - a.x);
  const expectedImage = new Uint8Array(crop.pixels.length);
  const actualImage = new Uint8Array(crop.pixels.length);
  let expectedPixels = 0;
  let actualPixels = 0;
  let both = 0;
  for (let row = 0; row < crop.height; row++) {
    for (let column = 0; column < crop.width; column++) {
      const index = row * crop.width + column;
      const px = (region.left + column + 0.5) / capture.width;
      const py = (region.top + row + 0.5) / capture.height;
      const u = ((px - a.x) * (d.y - a.y) - (py - a.y) * (d.x - a.x)) / det;
      const v = ((b.x - a.x) * (py - a.y) - (b.y - a.y) * (px - a.x)) / det;
      const inside = u >= 0 && u <= 1 && v >= 0 && v <= 1;
      const lx = x0 + u * (x1 - x0);
      const ly = y0 + v * (y1 - y0);
      const wanted =
        inside &&
        expected.some(
          ([ex0, ey0, ex1, ey1]) =>
            lx >= ex0 && lx <= ex1 && ly >= ey0 && ly <= ey1,
        );
      const offset = index * 4;
      const found =
        inside &&
        select([
          crop.pixels[offset]!,
          crop.pixels[offset + 1]!,
          crop.pixels[offset + 2]!,
        ]);
      expectedPixels += Number(wanted);
      actualPixels += Number(found);
      both += Number(wanted && found);
      const outside = inside ? 0 : 64;
      expectedImage.set(
        wanted ? [255, 255, 255, 255] : [outside, outside, outside, 255],
        offset,
      );
      actualImage.set(
        found ? [255, 255, 255, 255] : [outside, outside, outside, 255],
        offset,
      );
    }
  }
  const expectedFrame = { ...crop, pixels: expectedImage };
  const actualMask = { ...crop, pixels: actualImage };
  await Promise.all([
    writeFile(
      join(directory, `${name}-expected.png`),
      encodePng(expectedFrame),
    ),
    writeFile(join(directory, `${name}-actual.png`), encodePng(crop)),
    writeFile(
      join(directory, `${name}-actual-mask.png`),
      encodePng(actualMask),
    ),
    writeFile(
      join(directory, `${name}-diff.png`),
      encodePng(differenceImage(expectedFrame, actualMask, 128)),
    ),
  ]);
  const union = expectedPixels + actualPixels - both;
  return {
    expectedPixels,
    actualPixels,
    intersectionOverUnion: union === 0 ? 1 : both / union,
    precision: actualPixels === 0 ? 1 : both / actualPixels,
    recall: expectedPixels === 0 ? 1 : both / expectedPixels,
  };
}

/**
 * The settings panel scenario. `scrolling` checks wrapped notes, scroll bar
 * paint and nested scrolling through wheel steps and thumb drags; `shield`
 * opens the event log by wheel, checks that the log passes unused movement
 * outward, that the shield in front of PURGE changes only its own pixels
 * when lifted, and that PURGE clamps the log.
 */
function settingsPanel(part: "scrolling" | "shield") {
  return async (context: { readonly signal: AbortSignal }) => {
    await runBrowserEnvironment(
      `GUI settings panel ${part}`,
      {
        ...galleryEnvironment,
        evidenceParent: resolve(
          "target/integration-artifacts/gallery-gui/settings-panel",
        ),
      },
      context.signal,
      async (scenario) => {
        const g = await openGallery(scenario, { initialPage: "gui" });
        await g.page.waitForFunction(
          () =>
            document.querySelector<HTMLOutputElement>("#status")?.dataset
              .state === "ready",
        );
        const directory = scenario.evidence.directory;
        const evidence: Record<string, unknown> = {};
        const record = async (name: string, value: unknown) => {
          evidence[name] = value;
          await writeFile(
            join(directory, "settings-panel-evidence.json"),
            JSON.stringify(
              evidence,
              (_key, value) =>
                typeof value === "bigint" ? String(value) : value,
              2,
            ) + "\n",
          );
        };
        const text = async (selector: string) =>
          documentStatus(await g.page.locator(selector).textContent());
        const capture = async (label: string) => {
          const { frame } = await g.capture(label);
          assert.equal(frame.failedDrawCalls, 0);
          return { label, width: frame.width, height: frame.height };
        };
        // Hold the waveform still so completed frames differ only by the
        // inputs under test.
        await g.call(
          "galleryGuiAction",
          { role: "checkbox" },
          { kind: "toggle" },
        );
        await g.page.waitForFunction(
          () =>
            document.querySelector("#gui-autoscan")?.textContent === "standby",
        );
        await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));
        await g.page.mouse.move(1, 1);

        // Wrapped notes: a greedy word wrap of the notes into the columns
        // their fixed width holds at the bundled font's advance.
        const initial = await waitForGuiState(g);
        const notesLeaf = initial.texts.find(({ text }) => text.length > 80);
        assert.ok(notesLeaf, "missing notes");
        assert.equal(notesLeaf.text, NOTES_TEXT);
        const { glyph, line, lines, rect: notes } = NOTES;
        await record("notes", { rect: notes, metrics: FONT_METRICS, lines });
        assert.ok(
          lines.length >= 3,
          `notes wrapped into ${lines.length} lines`,
        );

        const views = telemetryScrollViews(initial);
        await record("scroll-views", views);
        assert.deepEqual(views.outer.offset, [0, 0]);
        assert.deepEqual(views.inner.offset, [0, 0]);
        assert.ok(views.outer.capacity[1] > 0.5);
        assert.ok(views.inner.capacity[1] > 1);
        near(views.outer.viewport[1], PANEL.telemetry[3], "telemetry viewport");
        near(views.inner.viewport[1], PANEL.eventLog[3], "event log viewport");

        // The event log is a VirtualList over the demo's whole history that
        // declares only its wanted range. Once the declared window follows
        // the committed offset, the loaded range and the capacity match an
        // independent layout of the declared entries, the sidebar shows the
        // range the list reported to React, and entries run newest first by
        // sequence number.
        const eventLogMatches = async (state: GalleryGuiState) => {
          const log = eventLog(state);
          const expected = expectedEventLog(log);
          const offset = log.inner.offset[1];
          const { first, last } = log.inner;
          const [wantedFirst, wantedLast] = expected.wanted(offset);
          assert.equal(log.inner.itemCount, log.itemCount);
          assert.equal(
            log.items.length,
            last - first,
            "the declared children do not fill the loaded range",
          );
          assert.deepEqual(
            log.items.map(({ index }) => index),
            Array.from({ length: last - first }, (_, at) => first + at),
            "the declared children are not the loaded items",
          );
          assert.ok(
            log.items.length < log.itemCount || log.itemCount <= 1,
            `the event log declares all ${log.itemCount} items`,
          );
          assert.deepEqual(
            [first, last],
            [wantedFirst, wantedLast],
            `loaded range at offset ${offset}`,
          );
          assert.ok(
            Math.abs(log.inner.capacity[1] - expected.capacity) < 1e-3,
            `capacity ${log.inner.capacity[1]} is not ${expected.capacity}`,
          );
          const sequences = log.items.map(({ text }) =>
            Number(/^(\d+) \/\/ /.exec(text)?.[1]),
          );
          sequences.forEach((sequence, at) =>
            assert.equal(sequence, sequences[0]! - at, "entries out of order"),
          );
          assert.equal(
            await text("#gui-events"),
            `items ${first}-${last} of ${log.itemCount}`,
          );
          return { log, expected };
        };
        const settledEventLog = async (
          name: string,
          predicate: (log: EventLog) => boolean = () => true,
        ) => {
          const deadline = performance.now() + 15_000;
          let failure: unknown = new Error(
            `${name}: the event log did not settle`,
          );
          while (performance.now() < deadline) {
            try {
              const state = await waitForGuiState(g);
              const settled = await eventLogMatches(state);
              if (predicate(settled.log)) {
                const { log, expected } = settled;
                await record(name, {
                  scroll: {
                    offset: log.inner.offset,
                    capacity: log.inner.capacity,
                    first: log.inner.first,
                    last: log.inner.last,
                    anchorIndex: log.inner.anchorIndex,
                    anchorOffset: log.inner.anchorOffset,
                  },
                  itemExtent: log.itemExtent,
                  overscan: log.overscan,
                  items: log.items.map((item, at) => ({
                    ...item,
                    position: expected.position(item.index),
                    extent: expected.extents[at],
                    lines: expected.lines[at],
                  })),
                  capacity: expected.capacity,
                });
                return { state, ...settled };
              }
              failure = new Error(
                `${name}: event log ${JSON.stringify(settled.log.inner, (_key, value) => (typeof value === "bigint" ? String(value) : value))}`,
              );
            } catch (error) {
              failure = error;
            }
            await new Promise((resolve) => setTimeout(resolve, 50));
          }
          throw failure;
        };
        const initialLog = await settledEventLog("event-log-top");
        assert.deepEqual(
          [initialLog.log.inner.first, initialLog.log.inner.anchorIndex],
          [0, 0],
        );
        assert.ok(
          initialLog.log.itemCount >= 90 &&
            initialLog.expected.capacity > 20 * PANEL.eventLog[3],
          "the event log history does not span many viewports",
        );
        assert.ok(
          initialLog.expected.extents.some(
            (extent) => Math.abs(extent - initialLog.log.itemExtent) > 0.05,
          ) &&
            new Set(initialLog.expected.lines.map(({ length }) => length))
              .size > 1,
          "declared entries all measure like one another or the estimate",
        );

        // Scroll bar frames: each visible thumb paints where an independent
        // geometry calculation puts it for the committed offsets.
        const barFrame = async (
          frame: Awaited<ReturnType<typeof capture>>,
          state: ScrollViews,
          name: string,
          withLog: boolean,
        ) => {
          const bars = {
            outer: expectedScrollBar(state.outer, PANEL.telemetry),
            // The outer offset moves the nested event log on screen.
            ...(withLog
              ? {
                  inner: expectedScrollBar(
                    state.inner,
                    eventLogViewport(state.outer.offset[1]),
                  ),
                }
              : {}),
          };
          const tracks = Object.fromEntries(
            Object.entries(bars).map(([bar, { track }]) => [
              bar,
              [track[0] - 0.01, track[1], track[2] + 0.01, track[3]],
            ]),
          ) as Record<string, LogicalRect>;
          const ink = await g.call<Record<string, LogicalRect>>(
            "galleryGuiInkBounds",
            frame.label,
            tracks,
          );
          const masks: Record<string, unknown> = {};
          for (const [bar, expected] of Object.entries(bars)) {
            const mask = await compareMask(
              g,
              directory,
              `${name}-${bar}-bar`,
              frame,
              [
                expected.track[0] - 0.03,
                expected.track[1],
                expected.track[2] + 0.03,
                expected.track[3],
              ],
              [expected.thumb],
              (pixel) => Math.max(...pixel) >= 150,
            );
            masks[bar] = mask;
            const [, top, , bottom] = ink[bar]!;
            assert.ok(
              Math.abs(top - expected.thumb[1]) < 0.03 &&
                Math.abs(bottom - expected.thumb[3]) < 0.03,
              `${name} ${bar} thumb spans ${top}..${bottom}, expected ${expected.thumb[1]}..${expected.thumb[3]}`,
            );
            assert.ok(
              mask.intersectionOverUnion > 0.5,
              `${name} ${bar} thumb mask: ${JSON.stringify(mask)}`,
            );
          }
          await record(name, { bars, ink, masks });
        };
        // Event log frames: within the part of the log the telemetry view
        // shows, entry ink lies on the independently wrapped lines of the
        // declared items at their offset positions, and each fully visible
        // line paints from its left edge to its last glyph, so a wrong item,
        // position or wrap would move or cut the ink.
        const eventFrame = async (
          frame: Awaited<ReturnType<typeof capture>>,
          settled: Awaited<ReturnType<typeof settledEventLog>>,
          name: string,
        ) => {
          const { log, expected } = settled;
          const [x, listTop, , listHeight] = eventLogViewport(
            log.outer.offset[1],
          );
          const offset = log.inner.offset[1];
          const [, outerTop, , outerHeight] = PANEL.telemetry;
          const top = Math.max(listTop, outerTop);
          const bottom = Math.min(listTop + listHeight, outerTop + outerHeight);
          const lineRects: LogicalRect[] = [];
          const lineEnds: Record<string, number> = {};
          const regions: Record<string, LogicalRect> = {};
          log.items.forEach((item, at) => {
            const itemTop = listTop + expected.position(item.index) - offset;
            const lines = expected.lines[at]!;
            // A one-line entry sits in its minimum-height box.
            const boxHeight =
              lines.length === 1 ? EVENT_MIN_HEIGHT : expected.line;
            lines.forEach((content, index) => {
              const y0 = itemTop + index * expected.line;
              const y1 = y0 + boxHeight;
              const clipped = [
                x,
                Math.max(y0, top),
                x + content.length * expected.glyph,
                Math.min(y1, bottom),
              ] as LogicalRect;
              if (clipped[3] > clipped[1]) lineRects.push(clipped);
              if (y0 >= top && y1 <= bottom) {
                const key = `item${item.index}-line${index}`;
                lineEnds[key] = x + content.length * expected.glyph;
                regions[key] = [x - 0.02, y0, x + EVENT_TEXT_WIDTH, y1];
              }
            });
          });
          assert.ok(
            Object.keys(regions).length >= 1,
            `${name}: no event log line is fully visible`,
          );
          const ink = await g.call<Record<string, LogicalRect>>(
            "galleryGuiInkBounds",
            frame.label,
            regions,
          );
          const logMask = await compareMask(
            g,
            directory,
            name,
            frame,
            [x - 0.03, top, x + EVENT_TEXT_WIDTH + 0.03, bottom],
            lineRects,
            (pixel) => Math.max(...pixel) >= 110,
          );
          await record(`${name}-frame`, { lineRects, lineEnds, ink, logMask });
          for (const [key, end] of Object.entries(lineEnds)) {
            const [start, , stop] = ink[key]!;
            assert.ok(
              start > x - 0.015 && start < x + expected.glyph,
              `${name} ${key} ink starts at ${start}, not ${x}`,
            );
            assert.ok(
              stop > end - expected.glyph && stop < end + 0.015,
              `${name} ${key} ink ends at ${stop}, not before ${end}`,
            );
          }
          assert.ok(logMask.actualPixels > 150, `${name}: no entry ink`);
          assert.ok(
            logMask.precision > 0.95,
            `${name}: entry ink escaped its wrapped lines: ${JSON.stringify(logMask)}`,
          );
        };

        const wheelAt = async (point: ProjectedPoint, notches: number) => {
          await g.page.mouse.move(point.clientX, point.clientY);
          await g.page.mouse.wheel(0, notches * WHEEL_NOTCH_PIXELS);
        };
        const until = async (
          name: string,
          predicate: (views: ScrollViews) => boolean,
        ) => {
          try {
            return telemetryScrollViews(
              await waitForGuiState(g, (state) =>
                predicate(telemetryScrollViews(state)),
              ),
            );
          } catch (failure) {
            const views = telemetryScrollViews(await waitForGuiState(g));
            await record(`${name}-timeout`, {
              outer: {
                offset: views.outer.offset,
                capacity: views.outer.capacity,
              },
              inner: {
                offset: views.inner.offset,
                capacity: views.inner.capacity,
              },
            });
            throw new Error(`${name}: scroll state did not settle`, {
              cause: failure,
            });
          }
        };
        const at = (actual: number, expected: number) =>
          Math.abs(actual - expected) < 1e-4;
        if (part === "scrolling") {
          await g.call("faceGalleryGuiToCamera");
          try {
            const detail = await capture("settings-bars-top");

            // Each wrapped line paints ink from the left edge to its last glyph,
            // once the telemetry view scrolls the notes into its viewport.
            const checkNotes = async (
              frame: typeof detail,
              outerOffset: number,
            ) => {
              const [nx, notesY, notesWidth, notesHeight] = notes;
              const ny = notesY - outerOffset;
              const lineRects = lines.map(
                (content, index) =>
                  [
                    nx,
                    ny + index * line,
                    nx + content.length * glyph,
                    ny + (index + 1) * line,
                  ] as LogicalRect,
              );
              const inkBounds = await g.call<Record<string, LogicalRect>>(
                "galleryGuiInkBounds",
                frame.label,
                Object.fromEntries(
                  lineRects.map(([x0, y0, , y1], index) => [
                    `line${index}`,
                    [x0 - 0.02, y0, x0 + notesWidth + 0.02, y1] as LogicalRect,
                  ]),
                ),
              );
              const notesMask = await compareMask(
                g,
                directory,
                "settings-notes",
                frame,
                [
                  nx - 0.03,
                  ny - 0.03,
                  nx + notesWidth + 0.03,
                  ny + notesHeight + 0.03,
                ],
                lineRects,
                (pixel) => Math.max(...pixel) >= 110,
              );
              await record("notes-frame", { lineRects, inkBounds, notesMask });
              lineRects.forEach(([x0, , x1], index) => {
                const ink = inkBounds[`line${index}`]!;
                assert.ok(
                  ink[0] > x0 - 0.015 && ink[0] < x0 + glyph,
                  `line ${index} ink starts at ${ink[0]}, not ${x0}`,
                );
                assert.ok(
                  ink[2] > x1 - glyph && ink[2] < x1 + 0.015,
                  `line ${index} ink ends at ${ink[2]}, not before ${x1}: ${lines[index]}`,
                );
              });
              assert.ok(notesMask.actualPixels > 200, "notes painted no ink");
              assert.ok(
                notesMask.precision > 0.97,
                `notes ink escaped its wrapped lines: ${JSON.stringify(notesMask)}`,
              );
            };

            await barFrame(detail, views, "settings-bars-top", false);

            const cameraBefore = transform(await g.inspect());
            // Each thumb drag records the routing outcomes that rejected or
            // cancelled its actions, beside the scroll state it reached.
            const dragThumb = async (
              bar: ReturnType<typeof expectedScrollBar>,
              toward: "start" | "end",
              name: string,
            ) => {
              const x = (bar.track[0] + bar.track[2]) / 2;
              const [from, to] = await projectContent(g, [
                [x, (bar.thumb[1] + bar.thumb[3]) / 2],
                [x, toward === "end" ? bar.track[3] + 0.3 : bar.track[1] - 0.3],
              ]);
              await g.call("observeGalleryGuiInput");
              try {
                await g.drag(
                  [from!.clientX, from!.clientY],
                  [to!.clientX, to!.clientY],
                );
              } finally {
                await record(
                  `${name}-input`,
                  await g.call("finishGalleryGuiInputObservation"),
                );
              }
            };

            // A wheel step over the readouts scrolls the telemetry view.
            const [tx, ty, tw, th] = PANEL.telemetry;
            const [readouts] = await projectContent(g, [
              [tx + tw * 0.4, ty + 0.15],
            ]);
            await wheelAt(readouts!, 1);
            const notched = await until("notched", ({ outer }) =>
              at(outer.offset[1], WHEEL_STEP),
            );
            assert.deepEqual(notched.inner.offset, [0, 0]);

            // Seven more steps bring the wrapped notes fully into view.
            for (let notch = 0; notch < 7; notch++) await wheelAt(readouts!, 1);
            const notesShown = await until("notes-shown", ({ outer }) =>
              at(outer.offset[1], 8 * WHEEL_STEP),
            );
            assert.deepEqual(notesShown.inner.offset, [0, 0]);
            const notesTop = notes[1] - notesShown.outer.offset[1];
            assert.ok(
              notesTop >= ty && notesTop + notes[3] <= ty + th,
              "the scrolled notes are clipped by the telemetry view",
            );
            await g.page.mouse.move(1, 1);
            await checkNotes(
              await capture("settings-notes"),
              notesShown.outer.offset[1],
            );

            // Dragging the telemetry thumb past its track end opens the log.
            await dragThumb(
              expectedScrollBar(notesShown.outer, PANEL.telemetry),
              "end",
              "telemetry-thumb",
            );
            const opened = await until("opened", ({ outer }) =>
              at(outer.offset[1], outer.capacity[1]),
            );
            assert.deepEqual(opened.inner.offset, [0, 0]);

            // The log sits inside the telemetry viewport once opened.
            const [ix, logTop, iw, ih] = eventLogViewport(
              opened.outer.offset[1],
            );
            assert.ok(
              logTop >= ty - 1e-4 && logTop + ih <= ty + th + 1e-4,
              `opened log ${logTop}..${logTop + ih} is clipped by the telemetry view`,
            );
            const [log] = await projectContent(g, [
              [ix + iw * 0.4, logTop + ih / 2],
            ]);

            // A step over the log scrolls the log alone.
            await wheelAt(log!, 1);
            const logNotched = await until("log-notched", ({ inner }) =>
              at(inner.offset[1], WHEEL_STEP),
            );
            assert.deepEqual(
              logNotched.outer.offset,
              opened.outer.offset,
              "a wheel over the event log also scrolled the telemetry view",
            );
            // The step moves the log within its first item: the anchor keeps
            // that item and the offset into it.
            const wheeled = await settledEventLog("event-log-wheeled", (log) =>
              at(log.inner.offset[1], WHEEL_STEP),
            );
            assert.deepEqual(
              [wheeled.log.inner.anchorIndex, wheeled.log.inner.first],
              [0, 0],
            );
            assert.ok(at(wheeled.log.inner.anchorOffset, WHEEL_STEP));
            await g.page.mouse.move(1, 1);
            const wheeledFrame = await capture("settings-log-wheeled");
            await barFrame(
              wheeledFrame,
              telemetryScrollViews(wheeled.state),
              "settings-log-wheeled-bars",
              true,
            );
            await eventFrame(wheeledFrame, wheeled, "settings-log-wheeled");

            // Dragging the log thumb past its track end scrolls it to its end.
            const logBar = (state: ScrollViews) =>
              expectedScrollBar(
                state.inner,
                eventLogViewport(state.outer.offset[1]),
              );
            // The declared window follows to the oldest entries; measuring
            // them keeps the offset at the end of the shortened content.
            await dragThumb(logBar(logNotched), "end", "log-thumb-end");
            await until("log-dragged", ({ inner }) =>
              at(inner.offset[1], inner.capacity[1]),
            );
            const dragged = await settledEventLog(
              "event-log-dragged",
              (log) =>
                log.inner.last === log.itemCount &&
                at(log.inner.offset[1], log.inner.capacity[1]),
            );
            const logEnd = telemetryScrollViews(dragged.state);
            assert.deepEqual(logEnd.outer.offset, opened.outer.offset);
            assert.ok(
              dragged.log.inner.first > 0 &&
                dragged.log.inner.anchorIndex > dragged.log.inner.first,
              `the dragged log did not move its window: ${JSON.stringify({ first: dragged.log.inner.first, anchor: dragged.log.inner.anchorIndex })}`,
            );
            await g.page.mouse.move(1, 1);
            const scrolledFrame = await capture("settings-bars-scrolled");
            await barFrame(
              scrolledFrame,
              logEnd,
              "settings-bars-scrolled",
              true,
            );
            await eventFrame(scrolledFrame, dragged, "settings-log-dragged");
            assert.ok(
              (await g.difference(detail.label, scrolledFrame.label))
                .changedPixels > 200,
              "nested scrolling did not visibly move the telemetry content",
            );

            // Back at its start, the log passes unused upward movement outward:
            // the telemetry view scrolls up a step while the log holds still.
            await dragThumb(logBar(logEnd), "start", "log-thumb-start");
            const logStart = await until("log-returned", ({ inner }) =>
              at(inner.offset[1], 0),
            );
            await settledEventLog(
              "event-log-returned",
              (log) => log.inner.first === 0 && log.inner.anchorIndex === 0,
            );
            assert.deepEqual(logStart.outer.offset, opened.outer.offset);
            await wheelAt(log!, -1);
            const passed = await until("passed", ({ outer }) =>
              at(outer.offset[1], opened.outer.offset[1] - WHEEL_STEP),
            );
            assert.deepEqual(passed.inner.offset, [0, 0]);
            await record("nested-scrolling", {
              notched: notched.outer.offset,
              opened: opened.outer.offset,
              logNotched: logNotched.inner.offset,
              logEnd: logEnd.inner.offset,
              passed: {
                outer: passed.outer.offset,
                inner: passed.inner.offset,
              },
            });
            await g.page.mouse.move(1, 1);
            assert.deepEqual(
              transform(await g.inspect()),
              cameraBefore,
              "GUI scrolling was also processed as a camera gesture",
            );
          } finally {
            await g.call("releaseGalleryGuiTransform");
          }
          assert.deepEqual(g.errors, []);
          return;
        }

        // Wheel steps over the readouts open the event log at the end of the
        // telemetry view. At its first item, the log passes unused upward
        // movement outward: the telemetry view scrolls up a step while the log
        // holds still.
        const [tx, ty, tw] = PANEL.telemetry;
        const [readouts] = await projectContent(g, [
          [tx + tw * 0.4, ty + 0.15],
        ]);
        await wheelAt(readouts!, 20);
        const opened = await until("opened", ({ outer }) =>
          at(outer.offset[1], outer.capacity[1]),
        );
        assert.deepEqual(opened.inner.offset, [0, 0]);
        {
          const [ix, iy, iw, ih] = eventLogViewport(opened.outer.offset[1]);
          const [log] = await projectContent(g, [[ix + iw * 0.4, iy + ih / 2]]);
          await wheelAt(log!, -1);
          const passed = await until("passed", ({ outer }) =>
            at(outer.offset[1], opened.outer.offset[1] - WHEEL_STEP),
          );
          assert.deepEqual(passed.inner.offset, [0, 0]);
          await record("passed-outward", {
            opened: opened.outer.offset,
            passed: { outer: passed.outer.offset, inner: passed.inner.offset },
          });
          await g.page.mouse.move(1, 1);
        }

        // The shield is scene geometry with picking geometry in front of
        // PURGE from the authored camera.
        const [purgeX, purgeY, purgeWidth, purgeHeight] = PANEL.purge;
        const [purgeCentre] = await projectContent(g, [
          [purgeX + purgeWidth / 2, purgeY + purgeHeight / 2],
        ]);
        assert.ok(purgeCentre);
        const shieldFace = await g.call<readonly ProjectedPoint[]>(
          "projectGalleryPoints",
          "gui-input-shield",
          [
            [-0.5, 0.5, 0.5],
            [0.5, 0.5, 0.5],
            [0.5, -0.5, 0.5],
            [-0.5, -0.5, 0.5],
          ],
        );
        const insideQuad = (
          quad: readonly ProjectedPoint[],
          { clientX, clientY }: ProjectedPoint,
        ) =>
          quad.every((corner, index) => {
            const next = quad[(index + 1) % quad.length]!;
            return (
              (next.clientX - corner.clientX) * (clientY - corner.clientY) -
                (next.clientY - corner.clientY) * (clientX - corner.clientX) >=
              0
            );
          }) ||
          quad.every((corner, index) => {
            const next = quad[(index + 1) % quad.length]!;
            return (
              (next.clientX - corner.clientX) * (clientY - corner.clientY) -
                (next.clientY - corner.clientY) * (clientX - corner.clientX) <=
              0
            );
          });
        assert.ok(
          insideQuad(shieldFace, purgeCentre),
          "PURGE is not behind the input shield from the authored camera",
        );
        assert.equal(await text("#gui-shield"), "armed");
        const armedFrame = await capture("settings-shield-armed");

        // Lifting the shield keeps the glass in front of PURGE and changes
        // pixels only on its projected box.
        await g.page.locator("#gui-shield-toggle").click();
        await g.page.waitForFunction(
          () => document.querySelector("#gui-shield")?.textContent === "lifted",
        );
        await g.page.mouse.move(1, 1);
        const liftedFrame = await capture("settings-shield-lifted");
        const lifted = await g.inspect();
        assert.ok(
          lifted.entities.some(
            ({ metadata }) => metadata.symbolicId === "gui-input-shield",
          ),
          "lifting the shield removed its glass",
        );
        // Only the shield's marks change between the armed and lifted
        // frames: every changed pixel lies on the projected box, and the
        // changed frame spans it.
        const shieldBox = convexHull(
          await g.call<readonly ProjectedPoint[]>(
            "projectGalleryPoints",
            "gui-input-shield",
            [-0.5, 0.5].flatMap((x) =>
              [-0.5, 0.5].flatMap((y) => [-0.5, 0.5].map((z) => [x, y, z])),
            ),
          ),
        );
        const faceBounds = [
          Math.min(...shieldBox.map(({ x }) => x)),
          Math.min(...shieldBox.map(({ y }) => y)),
          Math.max(...shieldBox.map(({ x }) => x)),
          Math.max(...shieldBox.map(({ y }) => y)),
        ] as const;
        const pad = 3 / armedFrame.width;
        const bounds = [
          faceBounds[0] - pad,
          faceBounds[1] - pad,
          faceBounds[2] + pad,
          faceBounds[3] + pad,
        ];
        const regions = await Promise.all(
          [armedFrame.label, liftedFrame.label].map((label) =>
            g.call<{
              left: number;
              top: number;
              width: number;
              height: number;
              pixels: string;
            }>("viewerCaptureRegionPixels", label, bounds),
          ),
        );
        const [before, after] = regions.map(decodeRegion) as [
          RgbaFrame,
          RgbaFrame,
        ];
        const { left, top } = regions[0]!;
        const face = shieldBox.map(
          ({ x, y }) =>
            ({
              x,
              y,
              clientX: x * armedFrame.width - left,
              clientY: y * armedFrame.height - top,
            }) as ProjectedPoint,
        );
        const expectedGlass = new Uint8Array(before.pixels.length);
        const changedGlass = new Uint8Array(before.pixels.length);
        const changedExtent = [Infinity, Infinity, -Infinity, -Infinity];
        let facePixels = 0;
        let changedPixels = 0;
        let changedOnFace = 0;
        for (let row = 0; row < before.height; row++) {
          for (let column = 0; column < before.width; column++) {
            const index = row * before.width + column;
            const inside = insideQuad(face, {
              x: 0,
              y: 0,
              clientX: column + 0.5,
              clientY: row + 0.5,
            });
            const changed = pixelDifference(before, after, index) > 24;
            facePixels += Number(inside);
            changedPixels += Number(changed);
            changedOnFace += Number(inside && changed);
            if (changed && inside) {
              changedExtent[0] = Math.min(changedExtent[0]!, column);
              changedExtent[1] = Math.min(changedExtent[1]!, row);
              changedExtent[2] = Math.max(changedExtent[2]!, column + 1);
              changedExtent[3] = Math.max(changedExtent[3]!, row + 1);
            }
            expectedGlass.set(
              inside ? [255, 255, 255, 255] : [0, 0, 0, 255],
              index * 4,
            );
            changedGlass.set(
              changed ? [255, 255, 255, 255] : [0, 0, 0, 255],
              index * 4,
            );
          }
        }
        const expectedFrame = { ...before, pixels: expectedGlass };
        const changedFrame = { ...before, pixels: changedGlass };
        await Promise.all([
          writeFile(
            join(directory, "settings-shield-expected.png"),
            encodePng(expectedFrame),
          ),
          writeFile(
            join(directory, "settings-shield-armed-crop.png"),
            encodePng(before),
          ),
          writeFile(
            join(directory, "settings-shield-actual.png"),
            encodePng(after),
          ),
          writeFile(
            join(directory, "settings-shield-actual-mask.png"),
            encodePng(changedFrame),
          ),
          writeFile(
            join(directory, "settings-shield-diff.png"),
            encodePng(differenceImage(expectedFrame, changedFrame, 128)),
          ),
        ]);
        const faceExtent = [
          Math.min(...face.map(({ clientX }) => clientX)),
          Math.min(...face.map(({ clientY }) => clientY)),
          Math.max(...face.map(({ clientX }) => clientX)),
          Math.max(...face.map(({ clientY }) => clientY)),
        ];
        const glass = {
          facePixels,
          changedPixels,
          precision: changedOnFace / Math.max(1, changedPixels),
          changedExtent,
          faceExtent,
        };
        await record("shield-glass", glass);
        assert.ok(facePixels > 400, "the shield covers too few pixels");
        assert.ok(
          changedPixels > 150,
          `lifting the shield barely changed its marks: ${JSON.stringify(glass)}`,
        );
        assert.ok(
          glass.precision > 0.95,
          `lifting the shield changed pixels off its glass: ${JSON.stringify(glass)}`,
        );
        changedExtent.forEach((value, side) =>
          assert.ok(
            Math.abs(value - faceExtent[side]!) < 2.5,
            `the changed shield frame does not span its face: ${JSON.stringify(glass)}`,
          ),
        );

        // Visual occlusion alone never blocks GUI input: PURGE takes the
        // click through the unmarked glass.
        await g.page.mouse.click(purgeCentre.clientX, purgeCentre.clientY);
        await g.page.waitForFunction(
          () =>
            document.querySelector("#gui-command")?.textContent ===
            "Log purged",
        );
        const purged = await waitForGuiState(g, (state) =>
          state.eventLog.items.some(({ text }) => text.endsWith("LOG PURGED")),
        );
        const purgedViews = telemetryScrollViews(purged);
        assert.equal(purgedViews.inner.capacity[1], 0);

        // PURGE leaves one entry; the runtime clamps the log's scroll
        // position to the remaining content and anchors it at the top.
        await settledEventLog(
          "event-log-purged",
          (log) =>
            log.itemCount === 1 &&
            log.inner.anchorIndex === 0 &&
            log.inner.anchorOffset === 0,
        );
        // With nothing left to scroll, a step over the log passes to the
        // telemetry view, which reaches its end with the log in view.
        {
          const [ix, iy, iw] = eventLogViewport(purgedViews.outer.offset[1]);
          const [over] = await projectContent(g, [[ix + iw * 0.4, iy + 0.15]]);
          await g.page.mouse.move(over!.clientX, over!.clientY);
          await g.page.mouse.wheel(0, 20 * WHEEL_NOTCH_PIXELS);
          await waitForGuiState(g, (state) => {
            const { outer } = telemetryScrollViews(state);
            return Math.abs(outer.offset[1] - outer.capacity[1]) < 1e-4;
          });
          await g.page.mouse.move(1, 1);
        }
        const emptied = await settledEventLog(
          "event-log-emptied",
          (log) =>
            log.itemCount === 1 &&
            log.inner.offset[1] === 0 &&
            log.outer.offset[1] === log.outer.capacity[1],
        );
        assert.ok(emptied.log.items[0]!.text.endsWith("LOG PURGED"));
        await g.call("faceGalleryGuiToCamera");
        try {
          const purgedFrame = await capture("settings-log-purged");
          await barFrame(
            purgedFrame,
            telemetryScrollViews(emptied.state),
            "settings-log-purged-bars",
            true,
          );
          await eventFrame(purgedFrame, emptied, "settings-log-purged");
        } finally {
          await g.call("releaseGalleryGuiTransform");
        }

        await g.page.locator("#gui-shield-toggle").click();
        await g.page.waitForFunction(
          () => document.querySelector("#gui-shield")?.textContent === "armed",
        );
        await g.page.mouse.move(1, 1);
        await capture("settings-panel-final");
        assert.deepEqual(g.errors, []);
      },
    );
  };
}

test(
  "Gallery GUI settings panel wraps notes and nests scrolling with scroll bars",
  { timeout: 180_000 },
  settingsPanel("scrolling"),
);

test(
  "Gallery GUI settings panel passes scrolling outward, keeps its input shield in front of PURGE and clamps the purged log",
  { timeout: 180_000 },
  settingsPanel("shield"),
);

test("Gallery GUI input shield blocks pointer and wheel input while armed", {
  skip: "IppCanvas fixes guiInput blockers when its physical input context opens and reports blocked routing to no application callback; the gallery cannot name the shield it mounts later, count blocked input or lift it (ipp-kmw5.11.2 dependency)",
}, async () => {
  // The armed shield is scene picking geometry marked as a blocker: a press
  // and a wheel step aimed at PURGE through the glass are blocked, reach
  // neither the panel nor the camera, and are observable as blocked input
  // that the event log records. Lifting the shield stops marking it and the
  // same click presses PURGE; re-arming blocks it again.
});
