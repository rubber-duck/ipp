import type {
  RenderStatisticsSnapshot,
  SurfaceCacheRecord,
} from "@ipp/client/diagnostics";
import type { AnimationControllerSnapshot, Inspection } from "@ipp/client";
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
  mask,
  pixelDifference,
  type RgbaFrame,
} from "./retained-gui-images.js";
import {
  EVENT_LOG,
  FONT_METRICS,
  LAYERS,
  NOTES,
  NOTES_TEXT,
  OVERLAYS,
  PANEL,
  WHEEL_STEP,
  assertRetainedControls,
  control,
  controlPoint,
  controlRect,
  controlRegion,
  controlValue,
  dashboardControl,
  expectedScrollBar,
  logical,
  overlayControl,
  projectContent,
  telemetryScrollViews,
  textWidth,
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
import {
  PANEL_ENTITY,
  PIXEL_COVERAGE_CANVAS_SHARE,
  SKIN_SETTLE_MS,
  assertPlanes,
  awaitStationIdle,
  decodeRegion,
  dynamicProperty,
  fieldsWith,
  liftInputShield,
  sceneEntity,
  waitForGuiState,
  type Gallery,
  type RegionStats,
} from "./gallery-gui-support.js";

const PANEL_WORLD = "gui-demo-panel";

/**
 * CSS pixels of one Chromium wheel notch. The browser adapter converts DOM
 * deltas to notches and scrolls each by the gallery's step, so the suites
 * dispatch whole notches.
 */
const WHEEL_NOTCH_PIXELS = 100;

/**
 * The station's nodes at the demo's initial 64% gain, strongest first, as
 * the node grid lists them after its first sync: each node's signal is its
 * full-gain strength scaled by 0.35 + 0.65 x gain, and nodes from 40% are
 * online.
 */
const NODE_STRENGTH = {
  alpha: 0.96,
  charlie: 0.88,
  hotel: 0.81,
  echo: 0.74,
  golf: 0.66,
  bravo: 0.58,
  delta: 0.47,
  foxtrot: 0.33,
} as const;

function nodeSignal(key: keyof typeof NODE_STRENGTH, gain: number): number {
  return Math.round(100 * NODE_STRENGTH[key] * (0.35 + 0.65 * gain));
}

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

const guiEnvironment = {
  ...galleryEnvironment,
  evidenceParent: resolve("target/integration-artifacts/gallery-gui"),
};

function guiEntity(inspection: Inspection) {
  return inspection.entities.find(
    ({ metadata }) => metadata.symbolicId === PANEL_ENTITY,
  );
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

/** The named dashboard controls and the restated rectangle of each. */
const DASHBOARD_CONTROLS: readonly GalleryGuiSelector[] = [
  { role: "scrollView", name: "TELEMETRY" },
  { role: "virtualList" },
  { role: "button", name: "EVENTS" },
  { role: "button", name: "SCENE" },
  { role: "button", name: "SCOPE" },
  { role: "button", name: "NODES" },
  { role: "button", name: "CONTROLS" },
  { role: "button", name: "COLOUR" },
  { role: "text", name: "FIND" },
  { role: "button", name: "CLEAR" },
  { role: "button", name: "Minimize" },
  { role: "button", name: "Close" },
  { role: "slider" },
  { role: "checkbox", name: "SCAN" },
  { role: "button", name: "PULSE" },
  { role: "text", name: "CALLSIGN" },
  { role: "button", name: "UPLINK" },
  { role: "button", name: "STOP" },
  { role: "button", name: "SYNC" },
  { role: "button", name: "PURGE" },
  { role: "button", name: "ADVANCED" },
  { role: "button", name: "CYAN" },
  { role: "button", name: "AMBER" },
  { role: "checkbox", name: "EXPLODE LAYERS" },
  { role: "checkbox", name: "REDUCED MOTION" },
];

/**
 * Every named control is evaluated where the restated layout puts it, and
 * the node grid's rows follow in signal order: a layout regression fails
 * here before any pixel is read.
 */
function assertDashboardLayout(state: GalleryGuiState) {
  for (const selector of DASHBOARD_CONTROLS) {
    const actual = control(state, selector).bounds;
    const expected = controlRect(selector);
    actual.forEach((value, index) =>
      assert.ok(
        Math.abs(value - expected[index]!) < 0.05,
        `${selector.role} ${selector.name ?? ""} lies at ${JSON.stringify(actual)}, not ${JSON.stringify(expected)}`,
      ),
    );
  }
}

test("Gallery runs a real GUI demo and cleans it up", {
  timeout: 300_000,
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
        canvasShare: PIXEL_COVERAGE_CANVAS_SHARE,
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
          (await g.call<string[]>("galleryWorlds")).some((name) =>
            name.startsWith(PANEL_WORLD + "/"),
          )
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
      // Samples of the scope above and below its middle band: the trace
      // reaches them only at higher gain.
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
      // The light the pulse's flat baseline adds near both ends of the
      // scope's zero line. At the oblique test camera the line is about a
      // pixel thick, so one pixel's level depends on where the line falls
      // within it; the green it adds over the whole column of pixels round
      // it, against the same pixels before the pulse, does not.
      const sampleWaveformBaseline = async (
        label: string,
        before: string,
        { width, height }: { readonly width: number; readonly height: number },
      ) => {
        const centres = await g.call<{ x: number; y: number }[]>(
          "projectGalleryGuiContent",
          [0.06, 0.15, 0.85, 0.94].map((fraction) => [
            gridX + gridWidth * fraction,
            gridY + gridHeight / 2,
          ]),
        );
        const rows = [-3, -2, -1, 0, 1, 2, 3];
        const column = centres.flatMap(({ x, y }) =>
          rows.map(
            (row) =>
              [
                (Math.floor(x * width) + 0.5) / width,
                (Math.floor(y * height) + row + 0.5) / height,
              ] as const,
          ),
        );
        const [lit, unlit] = await Promise.all(
          [label, before].map((capture) =>
            g.call<readonly (readonly number[])[]>(
              "sampleViewerCapture",
              capture,
              column,
            ),
          ),
        );
        return centres.map((_, group) =>
          rows.reduce((sum, _row, index) => {
            const pixel = group * rows.length + index;
            return sum + Math.max(0, lit![pixel]![1]! - unlit![pixel]![1]!);
          }, 0),
        );
      };
      // Two complete sine cycles at phase zero: alternating crests and
      // troughs, 60 drawing units high at full gain scaled into the scope.
      const sampleSineExtrema = async (label: string) => {
        const state = await waitForGui();
        const gain = controlValue(state, "slider");
        assert.equal(gain.kind, "scalar");
        const amplitude = ((60 * gridWidth) / 330) * (0.12 + 0.88 * gain.value);
        const fractions = [0.125, 0.375, 0.625, 0.875];
        const samples = await g.call<readonly (readonly number[])[]>(
          "sampleGalleryGuiCapture",
          label,
          fractions.flatMap((fraction) =>
            [-2, 0, 2].map((offset) => [
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
      // The page between the panels is translucent: a gap pixel is the page
      // colour at 86% over the scene the panel hides, both measured.
      const pageGap: readonly [number, number][] = [
        [PANEL.telemetryPanel[0] + PANEL.telemetryPanel[2] + 8, 300],
        [PANEL.nodesPanel[0] - 8, 300],
      ];
      const assertTranslucentPage = async (label: string) => {
        await g.call("overrideGalleryGuiTransform", { x: 1000 });
        try {
          await g.capture(`${label}-backdrop`);
        } finally {
          await g.call("releaseGalleryGuiTransform");
        }
        await g.capture(label);
        const painted = await g.call<number[][]>(
          "sampleGalleryGuiCapture",
          label,
          pageGap,
        );
        const behind = await g.call<number[][]>(
          "sampleGalleryGuiCapture",
          `${label}-backdrop`,
          pageGap,
        );
        // The token page colour, #011722, in sRGB levels.
        const page = [1, 23, 34];
        painted.forEach((pixel, index) => {
          const expected = page.map((level, channel) =>
            linearToSrgb(
              srgbToLinear(level) * 0.86 +
                srgbToLinear(behind[index]![channel]!) * 0.14,
            ),
          );
          pixel
            .slice(0, 3)
            .forEach((value, channel) =>
              assert.ok(
                Math.abs(value - expected[channel]!) < 6,
                `the page between panels is not the page colour at 86% over the scene: ${JSON.stringify({ painted, behind, expected })}`,
              ),
            );
        });
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
      // UPLINK's label, and the panel interior beside the button.
      const uplinkRegions = (label: string) => {
        const [x, y, width, height] = PANEL.uplink;
        const text = textWidth("UPLINK", 16);
        return regionStats(label, {
          fill: [
            x + (width - text) / 2,
            y + height / 2 - 6,
            x + (width + text) / 2,
            y + height / 2 + 6,
          ],
          gap: [x - 14, y + 8, x - 4, y + height - 8],
        });
      };
      // One detail capture after the transitions have had time to settle; a
      // part that stays on its previous sample fails the caller's colour
      // comparison instead of being retried.
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
      await assertTranslucentPage("gui-demo-page");
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
        !(await g.call<string[]>("galleryWorlds")).some((name) =>
          name.startsWith(PANEL_WORLD + "/"),
        );

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
        animationFor(loadingFrame.inspection, "gui-projector-dust").state,
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
        "switching",
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
        "switching",
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
      // The dashboard's controls; the closed overlays beside it keep theirs
      // hidden.
      assert.ok(
        prepared.controls
          .filter((candidate) => !overlayControl(candidate))
          .every(({ visible }) => visible),
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
      // This intentionally observes the staged scene before readiness.
      await g.inspect();
      assert.equal(
        documentStatus(await g.page.locator("#gui-autoscan").textContent()),
        "enabled",
        "staged GUI demo accepted pointer input before placement",
      );
      const resourcesLoadedMs = performance.now() - startupStarted;
      assert.equal(
        await g.page.locator("#status").getAttribute("data-state"),
        "switching",
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
        "switching",
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
          controls
            .filter((candidate) => !overlayControl(candidate))
            .every(({ available }) => available),
      );
      const firstReadyFrame = await g.capture("gui-demo-first-ready");
      assert.ok(firstReadyFrame.summary.coverage > 0.08);
      // The panel is ordinary Canvas content of an attached World presented
      // through the parent's Surface.
      assert.ok(initial.panelComponents.includes("FlatSurface"));
      assert.ok(initial.panelComponents.includes("WorldAttachment"));
      const page = initial.boxes.find(({ symbol }) => symbol === "gui-page");
      assert.ok(page, "the dashboard has no page");
      assert.ok(
        Math.abs(page.alpha * page.opacity - 0.86) < 1e-6,
        "the page must paint at effective alpha 0.86",
      );
      // The event log VirtualList nests inside the telemetry ScrollView.
      telemetryScrollViews(initial);

      // The station syncs its nodes once the panel shows: rows arrive in
      // signal order and the operation completes.
      const synced = await awaitStationIdle(g);
      assertDashboardLayout(synced);
      const rows = synced.controls.filter(({ symbol }) =>
        /^gui-nodes\/row\/[a-z]+$/.test(symbol ?? ""),
      );
      const keys = Object.keys(NODE_STRENGTH) as (keyof typeof NODE_STRENGTH)[];
      assert.deepEqual(
        rows.map(({ symbol }) => symbol!.split("/").pop()),
        keys,
        "the node grid's rows are not in signal order",
      );
      rows.forEach(({ bounds }, index) =>
        bounds.forEach((value, axis) =>
          assert.ok(
            Math.abs(value - PANEL.gridRow(index)[axis]!) < 0.05,
            `grid row ${index} lies at ${JSON.stringify(bounds)}`,
          ),
        ),
      );
      const cell = (key: string, column: string) =>
        synced.texts.find(
          ({ symbol }) => symbol === `gui-nodes/row/${key}/${column}/text`,
        )?.text;
      for (const key of keys) {
        const signal = nodeSignal(key, 0.64);
        assert.equal(cell(key, "signal"), `${signal}%`);
        assert.equal(cell(key, "status"), signal >= 40 ? "Online" : "Standby");
      }
      assert.equal(
        await g.page.locator("#gui-nodes").textContent(),
        "6 of 8 online",
      );

      assert.deepEqual(controlValue(synced, "checkbox", "SCAN"), {
        kind: "bool",
        value: true,
      });
      for (const name of ["EXPLODE LAYERS", "REDUCED MOTION"])
        assert.deepEqual(controlValue(synced, "checkbox", name), {
          kind: "bool",
          value: false,
        });
      assertScalarValue(synced, 0.64);
      assert.deepEqual(controlValue(synced, "text", "CALLSIGN"), {
        kind: "text",
        value: "VESPER-7",
      });
      for (const node of synced.controls.filter(
        (candidate) => !overlayControl(candidate),
      )) {
        assert.equal(node.visible, true);
        assert.equal(node.available, true);
        // Scanning holds the uplink.
        assert.equal(node.enabled, node.label !== "UPLINK", node.label);
      }

      const overview = await g.capture("gui-demo-overview");
      // Panel interiors are filled with the page colour.
      const panelPaint = await g.call<number[][]>(
        "sampleGalleryGuiCapture",
        "gui-demo-overview",
        [
          [PANEL.nodesPanel[0] + 144, 400],
          [PANEL.monitorPanel[0] + 200, 370],
        ],
      );
      assert.ok(
        panelPaint.every(
          ([r, g, b]) => r! < 40 && g! < 70 && b! < 90 && b! > r! + 10,
        ),
        `panel interiors lost their dark page fill: ${JSON.stringify(panelPaint)}`,
      );
      const overviewInspection = overview.inspection;
      for (const symbol of [
        "gui-projector-core",
        "gui-projector-trim",
        "gui-projector-emitter",
        "gui-projector-beam",
        "gui-projector-dust",
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
      // The panel lies flat: its Surface has no layer spacing.
      assert.equal(
        Number(
          fieldsWith(overviewInspection, PANEL_ENTITY, "width").layer_spacing,
        ),
        0,
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
        "playing SCAN did not move the curve inside its scope",
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
        ["waveform-pulse.ippd", "waveform.ippd"],
        "complex waveform curves share Surface drawing resources",
      );
      // The two traces are drawings; the scope's grid is a canvas paint.
      assert.equal(
        synced.drawings.length,
        2,
        "waveform geometry must not expand into hundreds of Canvas entities",
      );
      assert.ok(Number(overview.frame.statistics!.gui!.guiBatches) > 0);
      assert.ok(Number(overview.frame.statistics!.gui!.glyphPages) > 0);
      // Waveform, sweep, dust and layer spacing clips are resident.
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
      assert.ok(
        motionClips.size >= 5,
        `scene motion clips are not all resident: ${motionClips.size}`,
      );
      assert.ok(overview.frame.drawCalls > 0 && overview.frame.triangles > 0);
      assert.equal(overview.frame.failedDrawCalls, 0);
      assert.ok(overview.summary.coverage > 0.08);
      assert.deepEqual(overview.inspection.renderDiagnostics, []);
      assert.deepEqual(overviewPanel.renderDiagnostics, []);

      // UPLINK is held while SCAN runs: the default look's disabled state.
      const disabledUplink = control(synced, {
        role: "button",
        name: "UPLINK",
      });
      assert.equal(disabledUplink.enabled, false);
      const disabledRegions = await settledUplink("gui-detail-uplink-disabled");
      await recordRegions("uplink-disabled", disabledRegions);
      // A disabled label draws in the neutral line colour: no stronger in
      // green than in red by the accent's margin.
      assert.ok(
        disabledRegions.fill.max[1] - disabledRegions.fill.max[0] < 80,
        `disabled UPLINK label is lit: ${JSON.stringify(disabledRegions.fill)}`,
      );

      const cameraBeforeControls = transform(await g.inspect());
      const checkboxRegion = await controlRegion(g, {
        role: "checkbox",
        name: "SCAN",
      });
      const sliderRegion = await controlRegion(
        g,
        { role: "slider" },
        0.02,
        0.08,
      );
      const checkbox = await point("checkbox", "SCAN");
      await g.page.mouse.click(checkbox.clientX, checkbox.clientY);
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-autoscan")?.textContent === "standby",
      );
      let current = await waitForGui((state) => {
        const value = controlValue(state, "checkbox", "SCAN");
        return value.kind === "bool" && value.value === false;
      });
      await waitForWaveform((current) => scan(current).state === "paused");

      // SCAN standby enables UPLINK in place; its label lights.
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
        "enabling UPLINK did not change its captured label",
      );
      assert.ok(
        enabledRegions.fill.max[1] - enabledRegions.fill.max[0] > 100,
        `enabled UPLINK label is not lit: ${JSON.stringify(enabledRegions.fill)}`,
      );
      assert.ok(
        meanDifference(enabledRegions.gap, disabledRegions.gap) < 4,
        "UPLINK comparison background moved between captures",
      );

      const dustStart = await g.capture("gui-projector-dust-start");
      const dustController = animationFor(
        dustStart.inspection,
        "gui-projector-dust",
      );
      assert.equal(
        dustController.state,
        "playing",
        "SCAN must not pause the ambient dust",
      );
      const initialPhase = dynamicProperty(
        dustStart.inspection,
        "gui-projector-dust",
        "phase",
      );
      assert.equal(initialPhase.kind, "f32");
      await g.waitFor(
        (inspection) =>
          Number(
            dynamicProperty(inspection, "gui-projector-dust", "phase").value,
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
        loopEnd.scan.x < -0.95 * gridWidth,
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
      // Low gain drops nodes to standby and the station says so.
      await waitForPanel((inspection) =>
        inspection.entities.some(({ metadata }) =>
          metadata.symbolicId?.startsWith("gui-alert"),
        ),
      );
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-nodes")?.textContent === "1 of 8 online",
      );
      const lowGain = await waitForGui();
      assert.ok(
        lowGain.texts.some(
          ({ symbol, text }) =>
            symbol === "gui-alert/text" &&
            text === "Low gain: 7 nodes on standby.",
        ),
        "the station did not warn about low gain",
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
        `SCAN must paint two smooth sine cycles across the scope: ${JSON.stringify(sineExtrema)}`,
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
      assertRetainedControls(
        identityBeforeCamera,
        await waitForGui(),
        true,
        dashboardControl,
      );
      const obliqueFrame = await g.capture("gui-demo-camera-oblique");
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
        "gui-waveform-before-pulse",
        pulseFrame.frame,
      );
      await recordWaveform("pulse-baseline", { green: baselineSignal });
      assert.ok(
        baselineSignal.every((green) => green > 150),
        `manual pulse must join a visible baseline on both sides: ${JSON.stringify(baselineSignal)}`,
      );
      assert.ok(
        Math.abs(capturedWaveform.scan.opacity - 0.4) < 1e-6,
        "PULSE must preserve the visible paused sine trace",
      );
      // The pulse ring follows the burst's crossing on the Host clock and
      // completes with its check mark.
      await waitForPanel((inspection) =>
        inspection.entities.some(
          ({ metadata }) => metadata.symbolicId === "gui-pulse-ring/symbol",
        ),
      );
      const pulseToggle = await point("checkbox", "SCAN");
      await g.page.mouse.click(pulseToggle.clientX, pulseToggle.clientY);
      await waitForWaveform(
        (current) =>
          scan(current).state === "playing" && scan(current).time > 0.2,
      );
      // Running SCAN holds UPLINK again: its label returns to the disabled
      // sample.
      current = await waitForGui(
        (state) => !control(state, { role: "button", name: "UPLINK" }).enabled,
      );
      const redisabledRegions = await settledUplink(
        "gui-detail-uplink-redisabled",
      );
      await recordRegions("uplink-redisabled", redisabledRegions);
      assert.ok(
        meanDifference(redisabledRegions.fill, disabledRegions.fill) < 6,
        `re-disabled UPLINK label ${JSON.stringify(redisabledRegions.fill.mean)} is not the disabled label ${JSON.stringify(disabledRegions.fill.mean)}`,
      );
      await g.capture("gui-waveform-pulse-scan-toggled");
      const toggledPulse = await waveform();
      assert.ok(
        toggledPulse.scan.opacity > 0.95,
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
          current.pulse.x < capturedWaveform.pulse.x - 0.1 * gridWidth,
      );
      assert.equal(scan(advancedPulse).state, "playing");
      assert.equal(wavePulse(advancedPulse).state, "playing");
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
      // The second disabled to idle transition must return to the lit
      // label rather than keep the disabled sample.
      const reenabledRegions = await settledUplink(
        "gui-detail-uplink-reenabled",
      );
      await recordRegions("uplink-reenabled", reenabledRegions);
      assert.ok(
        meanDifference(reenabledRegions.fill, enabledRegions.fill) < 6,
        `re-enabled UPLINK label ${JSON.stringify(reenabledRegions.fill.mean)} is not the enabled label ${JSON.stringify(enabledRegions.fill.mean)}`,
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
      assertRetainedControls(
        beforeReset,
        await waitForGui(),
        true,
        dashboardControl,
      );
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
        `SCAN switch region did not visibly change: ${JSON.stringify(checkboxPaint)}`,
      );
      // SCAN is the switch look: its lit block sits at the rail's right end
      // while on and at its left end while off. Rows through the block,
      // inset from the rail ends, weigh each sample by its contrast with the
      // row median (the rail), so the ink centroid falls on the block's side.
      const [toggleX, toggleY, toggleWidth, toggleHeight] = PANEL.scan;
      const knobInk = async (label: string) => {
        const columns = 64;
        const rows = [0.3, 0.4, 0.5, 0.6, 0.7];
        const xs = Array.from(
          { length: columns },
          (_, column) =>
            toggleX + 6 + ((toggleWidth - 12) * column) / (columns - 1),
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
        assert.ok(weight > 0, `${label} shows no SCAN block`);
        return {
          centroid: moment / weight,
          centre: toggleX + toggleWidth / 2,
          weight,
        };
      };
      const knobOn = await knobInk("gui-demo-overview");
      const knobOff = await knobInk("gui-demo-controls-active");
      await recordRegions("scan-block", { on: knobOn, off: knobOff });
      assert.ok(
        knobOn.centroid > knobOn.centre + 8,
        `SCAN block is not at the right end while on: ${JSON.stringify(knobOn)}`,
      );
      assert.ok(
        knobOff.centroid < knobOff.centre - 8,
        `SCAN block is not at the left end while off: ${JSON.stringify(knobOff)}`,
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
      // The gain's value bar starts at the rail's start: samples just
      // inside the rail's left end, on its centre line, are lit for any
      // committed value above the minimum.
      const [sliderX, sliderY, , sliderHeight] = PANEL.gain;
      const fillColumn = [-1, 0, 1].map(
        (dy) => [sliderX + 6, sliderY + sliderHeight / 2 + dy] as const,
      );
      const fillSamples = await g.call<readonly (readonly number[])[]>(
        "sampleGalleryGuiCapture",
        "gui-demo-controls-active",
        fillColumn,
      );
      assert.ok(
        fillSamples.some(([, green, blue]) => green! > 180 && blue! > 200),
        `gain value bar does not start at the rail's start: ${JSON.stringify(fillSamples)}`,
      );
      assert.ok(
        (await g.difference("gui-demo-overview", "gui-demo-controls-active"))
          .changedPixels > 500,
        "trusted control input did not produce a visible GUI demo change",
      );

      // ACCENT re-themes the primary actions and the projector in place:
      // a machine client's press on the AMBER segment, while the callsign
      // editor keeps focus, rewrites the action theme's rows and recolours
      // the projector, and no control changes identity, value or focus.
      const identityBeforeAccent = current;
      const sceneBeforeAccent = await g.inspect();
      const waveformBeforeAccent = await waveform();
      const projectorBeforeAccent = {
        core: dynamicProperty(
          sceneBeforeAccent,
          "gui-projector-core",
          "accent",
        ),
        light: fieldsWith(
          sceneBeforeAccent,
          "gui-projector-light",
          "intensity",
        ),
        scanController: scan(waveformBeforeAccent).id,
        wavePulseController: wavePulse(waveformBeforeAccent).id,
      };
      const callsignBeforeAccent = control(identityBeforeAccent, {
        role: "text",
        name: "CALLSIGN",
      });
      assert.equal(callsignBeforeAccent.focused, true);
      const stillFocused = (state: GalleryGuiState) => {
        const focused = state.controls.filter(({ focused }) => focused);
        assert.equal(focused.length, 1, "re-theming moved or cleared focus");
        assert.equal(
          focused[0]!.target.entity,
          callsignBeforeAccent.target.entity,
        );
      };
      await g.capture("gui-demo-cyan");
      const amber = await g.call<GalleryGuiState>(
        "galleryGuiAction",
        { role: "button", name: "AMBER" },
        { kind: "press" },
      );
      await g.page.waitForFunction(
        () => document.querySelector("#gui-accent")?.textContent === "amber",
      );
      const amberState = await waitForGui();
      assertRetainedControls(
        identityBeforeAccent,
        amberState,
        false,
        dashboardControl,
      );
      stillFocused(amber);
      stillFocused(amberState);
      // The segmented control holds the selection in its items' fields.
      assert.equal(
        amberState.controls.find(({ label }) => label === "AMBER")?.target
          .entity,
        control(identityBeforeAccent, { role: "button", name: "AMBER" }).target
          .entity,
      );
      await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));
      const amberFrame = await g.capture("gui-demo-amber");
      const amberCore = dynamicProperty(
        amberFrame.inspection,
        "gui-projector-core",
        "accent",
      );
      const amberLight = fieldsWith(
        amberFrame.inspection,
        "gui-projector-light",
        "intensity",
      );
      assert.notDeepEqual(
        amberCore,
        projectorBeforeAccent.core,
        "AMBER did not recolor the projector cube",
      );
      assert.notDeepEqual(
        [amberLight.r, amberLight.g, amberLight.b],
        [
          projectorBeforeAccent.light.r,
          projectorBeforeAccent.light.g,
          projectorBeforeAccent.light.b,
        ],
        "AMBER did not recolor the projector light",
      );
      const amberWaveform = await waveform();
      assert.equal(
        scan(amberWaveform).id,
        projectorBeforeAccent.scanController,
        "re-theming replaced the waveform scan controller",
      );
      assert.equal(
        wavePulse(amberWaveform).id,
        projectorBeforeAccent.wavePulseController,
        "re-theming replaced the waveform pulse controller",
      );
      assert.equal(
        animationFor(amberFrame.inspection, "gui-projector-dust").id,
        dustController.id,
        "re-theming replaced the ambient dust controller",
      );
      const accentDifference = await g.difference(
        "gui-demo-cyan",
        "gui-demo-amber",
      );
      assert.ok(accentDifference.changedPixels > 1_000);
      assert.equal(amberFrame.frame.failedDrawCalls, 0);
      assert.deepEqual(amberFrame.inspection.renderDiagnostics, []);

      // Detailed regions, every rectangle from the restated layout.
      await withDetailView(async () => {
        const detail = await waitForGui();
        await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));
        await g.capture("gui-detail-amber");
        const evidence: Record<string, unknown> = {};
        const labelBand = (
          [x, y, width, height]: ContentRect,
          text: string,
        ) => {
          const half = textWidth(text, 16) / 2;
          return [
            x + width / 2 - half,
            y + height / 2 - 6,
            x + width / 2 + half,
            y + height / 2 + 6,
          ] as LogicalRect;
        };
        // PULSE's label is amber now: red outweighs blue in its ink.
        const amberLabel = await regionStats("gui-detail-amber", {
          pulse: labelBand(PANEL.pulse, "PULSE"),
        });
        evidence.amberPulse = amberLabel;
        assert.ok(
          amberLabel.pulse.max[0] > 200 &&
            amberLabel.pulse.max[0] > amberLabel.pulse.max[2] + 60,
          `PULSE did not take the amber look: ${JSON.stringify(amberLabel.pulse)}`,
        );

        // Labels sit centred in their buttons: the ink centroid of rows
        // through the label, weighed by contrast with the row median, falls
        // on the button's centre.
        for (const name of ["PULSE", "UPLINK", "CLEAR", "SYNC", "PURGE"]) {
          const [bx, by, bw, bh] = controlRect({ role: "button", name });
          const columns = 48;
          const labelXs = Array.from(
            { length: columns },
            (_, column) => bx + 6 + ((bw - 12) * column) / (columns - 1),
          );
          const labelSamples = await g.call<readonly (readonly number[])[]>(
            "sampleGalleryGuiCapture",
            "gui-detail-amber",
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
          assert.ok(offset < 2.5, `${name} label is off-centre by ${offset}`);
        }

        // Window control glyphs are centred in their docked buttons: ink
        // measured inside the buttons' lines.
        const inside = ([x, y, width, height]: ContentRect): LogicalRect => [
          x + 3,
          y + 3,
          x + width - 3,
          y + height - 3,
        ];
        const glyphs = await g.call<Record<string, LogicalRect>>(
          "galleryGuiInkBounds",
          "gui-detail-amber",
          {
            minimize: inside(PANEL.minimize),
            close: inside(PANEL.close),
          },
        );
        evidence.windowGlyphs = glyphs;
        for (const [name, box] of [
          ["minimize", PANEL.minimize],
          ["close", PANEL.close],
        ] as const) {
          const ink = glyphs[name]!;
          const inkCentre = [(ink[0] + ink[2]) / 2, (ink[1] + ink[3]) / 2];
          assert.ok(
            Math.abs(inkCentre[0]! - (box[0] + box[2] / 2)) < 2 &&
              Math.abs(inkCentre[1]! - (box[1] + box[3] / 2)) < 2,
            `${name} glyph is not centred: ${JSON.stringify(ink)}`,
          );
        }

        // The focused callsign editor: the lit line with its glow on every
        // side, its interior left dark.
        stillFocused(detail);
        const [ix, iy, iw, ih] = PANEL.callsign;
        const field = await regionStats<string>("gui-detail-amber", {
          ringTop: [ix + 0.35 * iw, iy - 0.5, ix + 0.65 * iw, iy + 2],
          ringBottom: [
            ix + 0.35 * iw,
            iy + ih - 2,
            ix + 0.65 * iw,
            iy + ih + 0.5,
          ],
          ringRight: [ix + iw - 2, iy + 0.3 * ih, ix + iw + 0.5, iy + 0.7 * ih],
          hollow: [ix + 0.7 * iw, iy + 0.3 * ih, ix + 0.9 * iw, iy + 0.7 * ih],
        });
        evidence.callsign = field;
        assert.ok(
          [field.ringTop!, field.ringBottom!, field.ringRight!].every(
            ({ max: [, green, blue] }) => green > 180 && blue > 180,
          ),
          `focused callsign lost its lit line: ${JSON.stringify(field)}`,
        );
        assert.ok(
          field.hollow!.mean[1] < 90,
          `focus filled the callsign interior: ${JSON.stringify(field.hollow)}`,
        );
        await recordRegions("detail", evidence);
      });

      // CYAN again through a real press on its segment.
      const cyanPoint = await point("button", "CYAN");
      await g.page.mouse.click(cyanPoint.clientX, cyanPoint.clientY);
      await g.page.waitForFunction(
        () => document.querySelector("#gui-accent")?.textContent === "cyan",
      );
      assertRetainedControls(
        identityBeforeAccent,
        await waitForGui(),
        false,
        dashboardControl,
      );

      await g.page.locator("#ipp-world-canvas").scrollIntoViewIfNeeded();
      // A wheel notch over the readouts scrolls the telemetry body.
      const [tx, ty, tw] = PANEL.telemetry;
      const [scroll] = await projectContent(g, [[tx + tw * 0.5, ty + 40]]);
      const cameraBeforeScroll = transform(await g.inspect());
      await g.capture("gui-demo-before-scroll");
      await g.page.mouse.move(scroll!.clientX, scroll!.clientY);
      const beforeNotch = telemetryScrollViews(await waitForGui()).outer;
      const notchTarget = Math.min(
        beforeNotch.offset[1] + WHEEL_STEP,
        beforeNotch.capacity[1],
      );
      assert.ok(
        notchTarget > beforeNotch.offset[1] + 20,
        `telemetry has no room for a wheel notch: ${JSON.stringify(beforeNotch.capacity)}`,
      );
      await g.page.mouse.wheel(0, WHEEL_NOTCH_PIXELS);
      const afterNotch = telemetryScrollViews(
        await waitForGui(
          (state) =>
            Math.abs(
              telemetryScrollViews(state).outer.offset[1] - notchTarget,
            ) < 1e-3,
        ),
      ).outer;
      await scenario.evidence.record("telemetry-wheel-notch", {
        before: { offset: beforeNotch.offset, capacity: beforeNotch.capacity },
        after: { offset: afterNotch.offset, capacity: afterNotch.capacity },
        viewport: afterNotch.viewport,
      });
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
        "the telemetry body did not visibly scroll",
      );
      await g.page.mouse.move(1, 1);

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
      assertRetainedControls(
        mountedPanel,
        await waitForGui(),
        true,
        dashboardControl,
      );
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
        "gui-projector-dust",
      );
      assert.notEqual(returnedDust.id, dustController.id);
      assert.equal(returnedDust.state, "playing");
      assert.deepEqual(controlValue(returned, "checkbox", "SCAN"), {
        kind: "bool",
        value: true,
      });
      assertScalarValue(returned, 0.64);
      assert.deepEqual(controlValue(returned, "text", "CALLSIGN"), {
        kind: "text",
        value: "VESPER-7",
      });
      assert.equal(await g.page.locator("#gui-accent").textContent(), "cyan");
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
 * Cached versus direct panel tolerance: at most 3.5% of panel pixels may
 * differ by more than 64 levels, and at least 95% of the pixels bright in
 * either presentation must reach the bright level less `textTolerance` in the
 * other. The band-one image resamples the panel once more than direct
 * drawing, which moves glyph and border edges by about one texel and softens
 * thin stems and one-pixel borders by up to about 20 levels. A hard
 * threshold flips those pixels at the dimmest labels and edges, and their
 * share of the bright mask then depends on how much unrelated bright content
 * the panel paints: the same flips scored 0.87 against outlined frames and
 * 0.84 against plain ones. Within the 20-level band, the cached images
 * agree at about 0.99 either way, while a one-pixel shift, a 3x3 blur or a
 * 15% dimming of the cached image scores below 0.9.
 */
const CACHE_COMPARISON = {
  channelThreshold: 64,
  maxChangedFraction: 0.035,
  textTolerance: 20,
  minTextAgreement: 0.95,
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

/** Brightest channel level of the panel's text, icon and glow pixels. */
const BRIGHT_LEVEL = 170;

/** Bright text, icon and glow pixels of the panel. */
function isBright(r: number, g: number, b: number): boolean {
  return Math.max(r, g, b) >= BRIGHT_LEVEL;
}

/**
 * Share of the pixels bright in either frame whose brightest channel in the
 * other frame is within `tolerance` levels below the bright level.
 */
function brightAgreement(
  expected: RgbaFrame,
  actual: RgbaFrame,
  tolerance: number,
): number {
  const level = (pixels: Uint8Array, index: number) =>
    Math.max(pixels[index]!, pixels[index + 1]!, pixels[index + 2]!);
  let bright = 0;
  let agreed = 0;
  for (let index = 0; index < expected.pixels.length; index += 4) {
    const direct = level(expected.pixels, index);
    const cached = level(actual.pixels, index);
    if (Math.max(direct, cached) < BRIGHT_LEVEL) continue;
    bright += 1;
    if (Math.min(direct, cached) >= BRIGHT_LEVEL - tolerance) agreed += 1;
  }
  return bright === 0 ? 1 : agreed / bright;
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
      const g = await openGallery(scenario, {
        initialPage: "gui",
        canvasShare: PIXEL_COVERAGE_CANVAS_SHARE,
      });
      await g.page.waitForFunction(
        () =>
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "ready",
      );
      // The station's first sync and its toast end before counts start.
      await awaitStationIdle(g);
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
          textAgreement: brightAgreement(
            expected!,
            actual!,
            CACHE_COMPARISON.textTolerance,
          ),
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
        { role: "checkbox", name: "SCAN" },
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
      const cyan = await compareWithDirect("surface-cache-cyan-band1", 1);

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

      // Panels stay whole on the base plane, so exploding a panel with no
      // overlay open separates nothing and keeps the cached image.
      const beforeLayers = await observeUntil(
        "surface-cache-before-layers",
        reused(1),
      );
      await g.page.locator("#gui-explode-toggle").click();
      await g.page.waitForFunction(
        () => document.querySelector("#gui-layers")?.textContent === "exploded",
      );
      await g.waitFor(
        (inspection) =>
          Math.abs(
            Number(
              fieldsWith(inspection, PANEL_ENTITY, "layer_spacing")
                .layer_spacing,
            ) - LAYERS.spacing,
          ) < 1e-4,
      );
      const whole = await observeUntil(
        "surface-cache-exploded-content",
        (entry) => entry?.mode === "layered",
      );
      assert.equal(cacheDelta(beforeLayers, whole).allocations, 0);
      // This explicit FlatSurface now separates its complete content panels.
      // The optional whole-Surface cache uses its layered direct fallback;
      // opening a notification retains that mode without allocating an image.
      await g.call(
        "galleryGuiAction",
        { role: "button", name: "CLEAR" },
        { kind: "press" },
      );
      const layered = await observeUntil(
        "surface-cache-exploded",
        (entry) => entry?.mode === "layered",
      );
      const heldLayers = await observe("surface-cache-exploded-held");
      assert.equal(heldLayers.record?.mode, "layered");
      assert.equal(cacheDelta(layered, heldLayers).repaints, 0);
      await g.page.locator("#gui-explode-toggle").click();
      await g.page.waitForFunction(
        () => document.querySelector("#gui-layers")?.textContent === "flat",
      );
      const flattened = await observeUntil(
        "surface-cache-flattened",
        reused(1),
      );
      assert.equal(cacheDelta(beforeLayers, flattened).allocations, 0);
      await record("layers", {
        whole: whole.record,
        layered: layered.record,
        flattened: flattened.record,
        delta: cacheDelta(beforeLayers, flattened),
      });
      await g.call(
        "galleryGuiAction",
        { role: "button", name: "Dismiss" },
        { kind: "press" },
      );
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

      // Re-theming is paint, not a resource change: it repaints at the
      // band's cadence and settles on the latest state.
      await g.call(
        "galleryGuiAction",
        { role: "button", name: "AMBER" },
        { kind: "press" },
      );
      await g.page.waitForFunction(
        () => document.querySelector("#gui-accent")?.textContent === "amber",
      );
      await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));
      const reskinned = await observeUntil(
        "surface-cache-amber-settled",
        reused(1),
      );
      const reskin = cacheDelta(previous, reskinned);
      const reskinSeconds =
        (reskinned.record!.paintedAtMs - previous.record!.paintedAtMs) / 1000;
      assert.equal(reskin.allocations, 0);
      assert.ok(reskin.repaints >= 1, "re-theming never repainted");
      assert.ok(
        reskin.repaints <= Math.ceil(reskinSeconds * CACHE_REFRESH_HZ) + 1,
        `re-theming repaints exceeded the ${CACHE_REFRESH_HZ} Hz cap: ${JSON.stringify({ reskin, reskinSeconds })}`,
      );
      await record("amber-repaints", { ...reskin, reskinSeconds });
      const amber = await compareWithDirect("surface-cache-amber-band1", 1);

      // Continuous scanning keeps the trace current without exceeding the
      // cap: the cached image repaints at most at the cap, and either keeps
      // repainting or the renderer presents the changing panel directly.
      previous = await observeUntil("surface-cache-before-scan", reused(1));
      await g.call(
        "galleryGuiAction",
        { role: "checkbox", name: "SCAN" },
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
        { role: "checkbox", name: "SCAN" },
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
        cyan,
        amber,
        durationMs: performance.now() - started,
      });
      assert.deepEqual(g.errors, []);
    },
  );
});

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
 * least text height) and the margins above and below. Positions, content
 * extent, capacity and the wanted range follow from those extents, the
 * viewport and the overscan.
 */
function expectedEventLog(log: EventLog) {
  const glyph = FONT_METRICS.advance * EVENT_LOG.textSize;
  const line = FONT_METRICS.lineHeight * EVENT_LOG.textSize;
  const columns = Math.floor(EVENT_LOG.textWidth / glyph + 1e-4);
  const estimate = log.itemExtent;
  const count = log.itemCount;
  const viewport = PANEL.eventLog[3];
  const first = log.items[0]?.index ?? 0;
  const lines = log.items.map(({ text }) => wrapColumns(text, columns));
  const extents = lines.map(
    (wrapped) =>
      Math.max(wrapped.length * line, EVENT_LOG.minText) + 2 * EVENT_LOG.margin,
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
    itemAt,
    content,
    capacity,
    wanted,
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
 * The dashboard's settings scenario. `scrolling` checks wrapped notes,
 * scroll bar paint and nested scrolling through wheel steps and thumb drags;
 * `shield` opens the telemetry body by wheel, checks that the log passes
 * unused movement outward, that the shield in front of PURGE changes only
 * its own pixels when lifted, that PURGE empties the node table and that
 * CLEAR clamps the log, with its toast painted and hit above the content.
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
        const g = await openGallery(scenario, {
          initialPage: "gui",
          canvasShare: PIXEL_COVERAGE_CANVAS_SHARE,
        });
        await g.page.waitForFunction(
          () =>
            document.querySelector<HTMLOutputElement>("#status")?.dataset
              .state === "ready",
        );
        await awaitStationIdle(g);
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
          { role: "checkbox", name: "SCAN" },
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
        const notesLeaf = initial.texts.find(
          ({ symbol }) => symbol === "gui-notes",
        );
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
        assert.ok(views.outer.capacity[1] > WHEEL_STEP);
        assert.ok(views.inner.capacity[1] > 1_000);
        assert.ok(
          Math.abs(views.outer.viewport[1] - PANEL.telemetry[3]) < 0.01,
          "telemetry viewport",
        );
        assert.ok(
          Math.abs(views.inner.viewport[1] - PANEL.eventLog[3]) < 0.01,
          "event log viewport",
        );
        // The body's content is its spaced children: readouts, notes, the
        // EVENTS division and the log, between an inset above and below.
        assert.ok(
          Math.abs(
            views.outer.capacity[1] -
              (PANEL.eventLog[1] +
                PANEL.eventLog[3] +
                16 -
                PANEL.telemetry[1] -
                PANEL.telemetry[3]),
          ) < 0.01,
          `telemetry capacity ${views.outer.capacity[1]}`,
        );

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
            Math.abs(log.inner.capacity[1] - expected.capacity) < 1e-2,
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
            initialLog.expected.capacity > 5 * PANEL.eventLog[3],
          "the event log history does not span many viewports",
        );
        assert.ok(
          initialLog.expected.extents.some(
            (extent) => Math.abs(extent - initialLog.log.itemExtent) > 2,
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
              [track[0] - 1, track[1], track[2] + 1, track[3]],
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
                expected.track[0] - 3,
                expected.track[1],
                expected.track[2] + 3,
                expected.track[3],
              ],
              [expected.thumb],
              (pixel) => Math.max(...pixel) >= 150,
            );
            masks[bar] = mask;
            // A bar whose content fits shows its track without a thumb.
            const view = bar === "outer" ? state.outer : state.inner;
            if (view.capacity[1] <= 0) {
              assert.equal(
                mask.actualPixels,
                0,
                `${name} ${bar} shows a thumb for content that fits`,
              );
              continue;
            }
            const [, top, , bottom] = ink[bar]!;
            assert.ok(
              Math.abs(top - expected.thumb[1]) < 3 &&
                Math.abs(bottom - expected.thumb[3]) < 3,
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
          const [listX, listTop, , listHeight] = eventLogViewport(
            log.outer.offset[1],
          );
          const x = listX + EVENT_LOG.textInset;
          const offset = log.inner.offset[1];
          const [, outerTop, , outerHeight] = PANEL.telemetry;
          // Inside the list frame's lines, and within the telemetry view.
          const top = Math.max(listTop + 2, outerTop);
          const bottom = Math.min(
            listTop + listHeight - 2,
            outerTop + outerHeight,
          );
          const lineRects: LogicalRect[] = [];
          const lineEnds: Record<string, number> = {};
          const regions: Record<string, LogicalRect> = {};
          log.items.forEach((item, at) => {
            const itemTop =
              listTop +
              expected.position(item.index) -
              offset +
              EVENT_LOG.margin;
            const lines = expected.lines[at]!;
            // A one-line entry sits in its least text box.
            const boxHeight =
              lines.length === 1 ? EVENT_LOG.minText : expected.line;
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
                regions[key] = [x - 2, y0, x + EVENT_LOG.textWidth, y1];
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
            // Clear of the list's scroll bar, which starts where the text
            // column ends.
            [x - 3, top, x + EVENT_LOG.textWidth - 2, bottom],
            lineRects,
            (pixel) => Math.max(...pixel) >= 110,
          );
          await record(`${name}-frame`, { lineRects, lineEnds, ink, logMask });
          for (const [key, end] of Object.entries(lineEnds)) {
            const [start, , stop] = ink[key]!;
            assert.ok(
              start > x - 1.5 && start < x + expected.glyph,
              `${name} ${key} ink starts at ${start}, not ${x}`,
            );
            assert.ok(
              stop > end - expected.glyph && stop < end + 1.5,
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
          Math.abs(actual - expected) < 1e-3;
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
            [x, toward === "end" ? bar.track[3] + 40 : bar.track[1] - 40],
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
        const logBar = (state: ScrollViews) =>
          expectedScrollBar(
            state.inner,
            eventLogViewport(state.outer.offset[1]),
          );
        const [tx, ty, tw, th] = PANEL.telemetry;
        // Over the readouts, clear of the nested log.
        const readoutsPoint = [tx + tw * 0.4, ty + 40] as const;
        if (part === "scrolling") {
          await g.call("faceGalleryGuiToCamera");
          try {
            const detail = await capture("settings-bars-top");

            // Each wrapped line paints ink from the left edge to its last glyph.
            const [nx, ny, notesWidth, notesHeight] = notes;
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
              detail.label,
              Object.fromEntries(
                lineRects.map(([x0, y0, , y1], index) => [
                  `line${index}`,
                  [x0 - 2, y0, x0 + notesWidth + 2, y1] as LogicalRect,
                ]),
              ),
            );
            const notesMask = await compareMask(
              g,
              directory,
              "settings-notes",
              detail,
              [nx - 3, ny - 3, nx + notesWidth + 3, ny + notesHeight + 3],
              lineRects,
              (pixel) => Math.max(...pixel) >= 110,
            );
            await record("notes-frame", { lineRects, inkBounds, notesMask });
            lineRects.forEach(([x0, , x1], index) => {
              const ink = inkBounds[`line${index}`]!;
              assert.ok(
                ink[0] > x0 - 1.5 && ink[0] < x0 + glyph,
                `line ${index} ink starts at ${ink[0]}, not ${x0}`,
              );
              assert.ok(
                ink[2] > x1 - glyph && ink[2] < x1 + 1.5,
                `line ${index} ink ends at ${ink[2]}, not before ${x1}: ${lines[index]}`,
              );
            });
            assert.ok(notesMask.actualPixels > 200, "notes painted no ink");
            assert.ok(
              notesMask.precision > 0.97,
              `notes ink escaped its wrapped lines: ${JSON.stringify(notesMask)}`,
            );

            await barFrame(detail, views, "settings-bars-top", false);

            const cameraBefore = transform(await g.inspect());
            // A wheel step over the readouts scrolls the telemetry body.
            const [readouts] = await projectContent(g, [readoutsPoint]);
            await wheelAt(readouts!, 1);
            const notched = await until("notched", ({ outer }) =>
              at(outer.offset[1], WHEEL_STEP),
            );
            assert.deepEqual(notched.inner.offset, [0, 0]);

            // Dragging the telemetry thumb past its track end shows the
            // whole log.
            await dragThumb(
              expectedScrollBar(notched.outer, PANEL.telemetry),
              "end",
              "telemetry-thumb",
            );
            const opened = await until("opened", ({ outer }) =>
              at(outer.offset[1], outer.capacity[1]),
            );
            assert.deepEqual(opened.inner.offset, [0, 0]);
            const [ix, logTop, iw, ih] = eventLogViewport(
              opened.outer.offset[1],
            );
            assert.ok(
              logTop >= ty - 1e-3 && logTop + ih <= ty + th + 1e-3,
              `opened log ${logTop}..${logTop + ih} is clipped by the telemetry view`,
            );
            const [logPoint] = await projectContent(g, [
              [ix + iw * 0.4, logTop + ih / 2],
            ]);

            // A step over the log scrolls the log alone.
            await wheelAt(logPoint!, 1);
            const logNotched = await until("log-notched", ({ inner }) =>
              at(inner.offset[1], WHEEL_STEP),
            );
            assert.deepEqual(
              logNotched.outer.offset,
              opened.outer.offset,
              "a wheel over the event log also scrolled the telemetry view",
            );
            // The anchor names the item the offset falls in and the offset
            // into it, from the independent layout.
            const wheeled = await settledEventLog("event-log-wheeled", (log) =>
              at(log.inner.offset[1], WHEEL_STEP),
            );
            const anchor = wheeled.expected.itemAt(WHEEL_STEP);
            assert.deepEqual(
              [wheeled.log.inner.anchorIndex, wheeled.log.inner.first],
              [anchor, 0],
            );
            assert.ok(
              Math.abs(
                wheeled.log.inner.anchorOffset -
                  (WHEEL_STEP - wheeled.expected.position(anchor)),
              ) < 1e-2,
            );
            await g.page.mouse.move(1, 1);
            const wheeledFrame = await capture("settings-log-wheeled");
            await barFrame(
              wheeledFrame,
              telemetryScrollViews(wheeled.state),
              "settings-log-wheeled-bars",
              true,
            );
            await eventFrame(wheeledFrame, wheeled, "settings-log-wheeled");

            // Dragging the log thumb past its track end scrolls it to its
            // end; the declared window follows to the oldest entries, and
            // measuring them keeps the offset at the end of the shortened
            // content.
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

            // Back at its start, the log passes unused upward movement
            // outward: the telemetry view scrolls up a step while the log
            // holds still.
            await dragThumb(logBar(logEnd), "start", "log-thumb-start");
            const logStart = await until("log-returned", ({ inner }) =>
              at(inner.offset[1], 0),
            );
            await settledEventLog(
              "event-log-returned",
              (log) => log.inner.first === 0 && log.inner.anchorIndex === 0,
            );
            assert.deepEqual(logStart.outer.offset, opened.outer.offset);
            await wheelAt(logPoint!, -1);
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

        // A wheel step over the readouts opens the telemetry body. At its
        // first item, the log passes unused upward movement outward: the
        // telemetry view scrolls back up while the log holds still.
        {
          const [readouts] = await projectContent(g, [readoutsPoint]);
          await wheelAt(readouts!, 1);
          const opened = await until("opened", ({ outer }) =>
            at(outer.offset[1], WHEEL_STEP),
          );
          assert.deepEqual(opened.inner.offset, [0, 0]);
          const [ix, iy, iw] = eventLogViewport(opened.outer.offset[1]);
          const [log] = await projectContent(g, [[ix + iw * 0.4, iy + 60]]);
          await wheelAt(log!, -1);
          const passed = await until("passed", ({ outer }) =>
            at(outer.offset[1], 0),
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
        // click through the unmarked glass and asks for confirmation in a
        // modal dialog centred on the canvas, focus on Cancel.
        await g.page.mouse.click(purgeCentre.clientX, purgeCentre.clientY);
        const asked = await waitForGuiState(g, (state) =>
          state.overlays.some(
            ({ symbol, visible }) => symbol === OVERLAYS.dialog && visible,
          ),
        );
        const dialogButton = (name: "cancel" | "action") =>
          control(asked, {
            role: "button",
            symbol: `${OVERLAYS.dialog}/${name}`,
          });
        for (const name of ["cancel", "action"] as const)
          dialogButton(name).bounds.forEach((value, axis) =>
            assert.ok(
              Math.abs(value - PANEL.dialog[name][axis]!) < 0.05,
              `the dialog's ${name} lies at ${JSON.stringify(dialogButton(name).bounds)}`,
            ),
          );
        assert.equal(dialogButton("cancel").focused, true);
        assert.equal(dialogButton("action").label, "Purge");
        // The amber action confirms; the node table empties row by row down
        // to its empty state.
        const [confirm] = await projectContent(g, [
          [
            PANEL.dialog.action[0] + PANEL.dialog.action[2] / 2,
            PANEL.dialog.action[1] + PANEL.dialog.action[3] / 2,
          ],
        ]);
        await g.page.mouse.click(confirm!.clientX, confirm!.clientY);
        await g.page.waitForFunction(
          () =>
            document.querySelector("#gui-operation")?.textContent ===
            "Node purge: complete",
        );
        const purged = await waitForGuiState(g, (state) =>
          state.texts.some(
            ({ symbol, text }) =>
              symbol === "gui-nodes/body/empty/text" && text === "No records",
          ),
        );
        assert.equal(await text("#gui-nodes"), "0 of 0 online");
        assert.ok(
          !purged.controls.some(({ symbol }) =>
            /^gui-nodes\/row\//.test(symbol ?? ""),
          ),
          "PURGE left node rows",
        );
        assert.equal(
          control(purged, { role: "button", name: "PURGE" }).enabled,
          false,
          "PURGE stays available over an empty table",
        );

        // CLEAR leaves one entry; the runtime clamps the log's scroll
        // position to the remaining content and anchors it at the top. Its
        // toast offers UNDO.
        const clear = await controlPoint(g, { role: "button", name: "CLEAR" });
        await g.page.mouse.click(clear.clientX, clear.clientY);
        await g.page.waitForFunction(
          () =>
            document.querySelector("#gui-command")?.textContent ===
            "Log cleared",
        );
        const cleared = await waitForGuiState(g, (state) =>
          state.eventLog.items.some(({ text }) => text.endsWith("LOG CLEARED")),
        );
        assert.equal(telemetryScrollViews(cleared).inner.capacity[1], 0);
        await settledEventLog(
          "event-log-cleared",
          (log) =>
            log.itemCount === 1 &&
            log.inner.anchorIndex === 0 &&
            log.inner.anchorOffset === 0,
        );
        await g.page.mouse.move(1, 1);
        await g.call("faceGalleryGuiToCamera");
        try {
          // Show the whole cleared log: the telemetry thumb to its end.
          const views = telemetryScrollViews(await waitForGuiState(g));
          await dragThumb(
            expectedScrollBar(views.outer, PANEL.telemetry),
            "end",
            "telemetry-thumb-cleared",
          );
          await until("cleared-opened", ({ outer }) =>
            at(outer.offset[1], outer.capacity[1]),
          );
          const emptied = await settledEventLog(
            "event-log-emptied",
            (log) =>
              log.itemCount === 1 &&
              log.inner.offset[1] === 0 &&
              at(log.outer.offset[1], log.outer.capacity[1]),
          );
          assert.ok(emptied.log.items[0]!.text.endsWith("LOG CLEARED"));
          await g.page.mouse.move(1, 1);
          const clearedFrame = await capture("settings-log-cleared");
          await barFrame(
            clearedFrame,
            telemetryScrollViews(emptied.state),
            "settings-log-cleared-bars",
            true,
          );
          await eventFrame(clearedFrame, emptied, "settings-log-cleared");

          // Toasts float above the content: the CLEAR toast, last in the
          // stack, covers the REDUCED MOTION checkbox. Its opaque interior
          // hides the checkbox's line, and a press there reaches the toast,
          // not the checkbox.
          const state = await waitForGuiState(g);
          const toasts = state.controls.filter(({ symbol }) =>
            /^gui-toasts\/[^/]+$/.test(symbol ?? ""),
          );
          assert.deepEqual(
            toasts.map(({ label }) => label),
            ["Nodes purged.", "Log cleared."],
          );
          toasts.forEach(({ bounds }, index) =>
            bounds.forEach((value, axis) =>
              assert.ok(
                Math.abs(value - PANEL.toast(index, 2)[axis]!) < 0.05,
                `toast ${index} lies at ${JSON.stringify(bounds)}`,
              ),
            ),
          );
          const [mx, my, , mh] = PANEL.reducedMotion;
          const checkboxLine: LogicalRect = [mx, my + 4, mx + 3, my + mh - 4];
          const covered = await g.call<Record<string, RegionStats>>(
            "galleryGuiRegionStats",
            clearedFrame.label,
            { line: checkboxLine },
          );
          await record("toast-cover", covered);
          assert.ok(
            covered.line!.max[2] < 80,
            `the toast shows the checkbox beneath it: ${JSON.stringify(covered)}`,
          );
        } finally {
          await g.call("releaseGalleryGuiTransform");
        }
        const motion = await controlPoint(g, {
          role: "checkbox",
          name: "REDUCED MOTION",
        });
        await g.page.mouse.click(motion.clientX, motion.clientY);
        await g.settle();
        assert.deepEqual(
          controlValue(await waitForGuiState(g), "checkbox", "REDUCED MOTION"),
          { kind: "bool", value: false },
          "a press on the toast reached the checkbox beneath it",
        );

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
  "Gallery GUI settings panel passes scrolling outward, keeps its input shield in front of PURGE, empties the node table and clamps the cleared log under its toast",
  { timeout: 180_000 },
  settingsPanel("shield"),
);

test("Gallery GUI keeps its panels whole when the layers explode and stays usable", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "GUI exploded layers",
    {
      ...galleryEnvironment,
      evidenceParent: resolve(
        "target/integration-artifacts/gallery-gui/exploded",
      ),
    },
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario, {
        initialPage: "gui",
        canvasShare: PIXEL_COVERAGE_CANVAS_SHARE,
      });
      await g.page.waitForFunction(
        () =>
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "ready",
      );
      await awaitStationIdle(g);
      // Static frames: SCAN off.
      await g.call(
        "galleryGuiAction",
        { role: "checkbox", name: "SCAN" },
        { kind: "toggle" },
      );
      await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));

      const ordinaryCamera = transform(await g.inspect());
      const flatProjection = await g.call("galleryGuiProjection");
      const flat = await g.capture("layers-flat");
      assert.equal(flat.frame.failedDrawCalls, 0);

      // The sidebar writes the EXPLODE LAYERS switch, whose value starts
      // the Surface's layer spacing animation on the Host clock.
      await g.page.locator("#gui-explode-toggle").click();
      await g.page.waitForFunction(
        () => document.querySelector("#gui-layers")?.textContent === "exploded",
      );
      assert.deepEqual(
        controlValue(await waitForGuiState(g), "checkbox", "EXPLODE LAYERS"),
        { kind: "bool", value: true },
      );
      await g.waitFor(
        (inspection) =>
          Math.abs(
            Number(
              fieldsWith(inspection, PANEL_ENTITY, "layer_spacing")
                .layer_spacing,
            ) - LAYERS.spacing,
          ) < 1e-4,
      );
      await g.page.mouse.move(1, 1);
      const exploded = await g.capture("layers-exploded");
      assert.equal(exploded.frame.failedDrawCalls, 0);
      const inspectionCamera = transform(exploded.inspection);
      assert.ok(
        Math.hypot(
          Number(inspectionCamera.x) - Number(ordinaryCamera.x),
          Number(inspectionCamera.z) - Number(ordinaryCamera.z),
        ) > 2,
        "EXPLODE did not open an inspection camera",
      );
      await g.page.screenshot({
        path: resolve(scenario.evidence.directory, "layers-exploded-page.png"),
        fullPage: true,
      });
      await scenario.evidence.record("inspection-camera", {
        ordinaryCamera,
        inspectionCamera,
      });

      // Panels stay whole: a frame and the content it holds stay on the
      // same occupied rank together.
      const shifts = await assertPlanes(
        g,
        {
          label: "layers-flat",
          frame: flat.frame,
          projection: flatProjection,
        },
        { label: "layers-exploded", frame: exploded.frame },
        {
          frame: {
            // TELEMETRY's top-left corner accent, in a small window clear
            // of the header text beside it.
            at: [PANEL.telemetryPanel[0] + 1.5, PANEL.telemetryPanel[1] + 1.5],
            plane: LAYERS.telemetry,
            radius: 8,
          },
          advanced: {
            at: [PANEL.advanced[0] + 1.5, PANEL.advanced[1] + 1.5],
            plane: LAYERS.advanced,
            radius: 8,
          },
          content: {
            // The gain readout's display digits.
            at: [
              PANEL.gainReadout[0] + 20,
              PANEL.gainReadout[1] + PANEL.gainReadout[3] / 2,
            ],
            plane: LAYERS.panel,
          },
        },
      );
      await scenario.evidence.record("layer-shifts", shifts);

      const toggleShield = async () => {
        const previous = await g.page
          .locator('textarea[data-ipp-native-text="true"]')
          .elementHandle();
        assert.ok(previous, "the physical input context has no native bridge");
        try {
          await g.page.locator("#gui-shield-toggle").click();
          await g.page.waitForFunction((old) => {
            const current = document.querySelector(
              'textarea[data-ipp-native-text="true"]',
            );
            return current !== old && current?.isConnected;
          }, previous);
        } finally {
          await previous.dispose();
        }
      };
      const shieldProbe = async (label: string, requireSide = true) => {
        const [x, y, width, height] = PANEL.purge;
        const at = [[x + width / 2, y + height / 2]] as const;
        const [button] = await projectContent(
          g,
          at,
          LAYERS.workbench * LAYERS.spacing,
        );
        const samples = Array.from(
          { length: 55 },
          (_, index) =>
            [
              x + (width * (index % 11)) / 10,
              y + (height * Math.floor(index / 11)) / 4,
            ] as const,
        );
        type CoverRay = {
          point: readonly [number, number];
          target: readonly number[];
          origin: readonly number[];
          near: number;
          far: number;
          axis: number;
          front: readonly number[];
        };
        const rays = await g.call<readonly CoverRay[]>(
          "galleryShieldRays",
          samples,
          LAYERS.workbench * LAYERS.spacing,
        );
        const enclosed = rays.every(({ target }) =>
          target.every((coordinate) => Math.abs(coordinate) <= 0.5 + 1e-4),
        );
        if (!enclosed) {
          await scenario.evidence.record("shield-enclosure-failure", {
            label,
            rays,
          });
          await g.capture(`${label}-shield-enclosure-failure`);
        }
        assert.ok(
          enclosed,
          "the cover does not enclose the entire curved PURGE rectangle",
        );
        const side = rays.find(
          (ray) =>
            ray.point[0] > x + 1 &&
            ray.point[0] < x + width - 1 &&
            ray.point[1] > y + 1 &&
            ray.point[1] < y + height - 1 &&
            ray.axis !== 2 &&
            ray.near >= 0 &&
            ray.near < 1 &&
            ray.far >= 1 &&
            (Math.abs(ray.front[0]!) > 0.5 || Math.abs(ray.front[1]!) > 0.5),
        );
        if (requireSide)
          assert.ok(
            side,
            "no near-edge PURGE ray misses the cap and enters a side wall",
          );
        const [sidePress] = await projectContent(
          g,
          [side?.point ?? at[0]],
          LAYERS.workbench * LAYERS.spacing,
        );
        await scenario.evidence.record("shield-ray-oracle", {
          label,
          button,
          side,
          rays,
        });
        const before = await waitForGuiState(g);
        assert.ok(
          !before.overlays.some(
            (overlay) => overlay.symbol === OVERLAYS.dialog && overlay.open,
          ),
        );
        await g.page.mouse.click(button!.clientX, button!.clientY);
        const blocked = await g.capture(`${label}-shield-blocked`);
        assert.equal(blocked.frame.failedDrawCalls, 0);
        assert.ok(
          !(await waitForGuiState(g)).overlays.some(
            (overlay) => overlay.symbol === OVERLAYS.dialog && overlay.open,
          ),
          "armed glass let the physical PURGE press through",
        );
        await g.page.mouse.click(sidePress!.clientX, sidePress!.clientY);
        const sideFrame = await g.capture(`${label}-shield-side-blocked`);
        if (side) {
          const wall = [0, 0, 0];
          wall[side.axis] = Math.sign(side.origin[side.axis]!) * 0.5;
          const [wallPoint] = await g.call<readonly ProjectedPoint[]>(
            "projectGalleryPoints",
            "gui-input-shield",
            [wall],
          );
          const region = await g.call<{
            width: number;
            height: number;
            pixels: string;
          }>("viewerCaptureRegionPixels", `${label}-shield-side-blocked`, [
            wallPoint!.x - 1 / sideFrame.frame.width,
            wallPoint!.y - 1 / sideFrame.frame.height,
            wallPoint!.x + 1 / sideFrame.frame.width,
            wallPoint!.y + 1 / sideFrame.frame.height,
          ]);
          const pixels = decodeRegion(region).pixels;
          let amber = 0;
          for (let i = 0; i < pixels.length; i += 4)
            if (
              pixels[i]! > 100 &&
              pixels[i]! > pixels[i + 1]! * 1.12 &&
              pixels[i + 1]! > pixels[i + 2]! * 1.4
            )
              amber++;
          assert.ok(
            amber >= 1,
            "the camera-facing side wall has no amber pixels",
          );
          await scenario.evidence.record("shield-wall-pixels", {
            label,
            wallPoint,
            amber,
            pixels: [...pixels],
          });
        }
        assert.ok(
          !(await waitForGuiState(g)).overlays.some(
            (overlay) => overlay.symbol === OVERLAYS.dialog && overlay.open,
          ),
          "armed cover admitted a side-wall ray",
        );
        await toggleShield();
        // The replacement native bridge proves close/open completed; the
        // frame records lifted presentation before its physical press.
        await g.capture(`${label}-shield-lifted-ready`);
        await g.page.mouse.click(sidePress!.clientX, sidePress!.clientY);
        await waitForGuiState(g, (state) =>
          state.overlays.some(
            (overlay) => overlay.symbol === OVERLAYS.dialog && overlay.open,
          ),
        );
        const admitted = await g.capture(`${label}-shield-lifted`);
        assert.equal(admitted.frame.failedDrawCalls, 0);
        const [cancel] = await projectContent(
          g,
          [
            [
              PANEL.dialog.cancel[0] + PANEL.dialog.cancel[2] / 2,
              PANEL.dialog.cancel[1] + PANEL.dialog.cancel[3] / 2,
            ],
          ],
          LAYERS.dialog * LAYERS.spacing,
        );
        await g.page.mouse.click(cancel!.clientX, cancel!.clientY);
        await waitForGuiState(
          g,
          (state) =>
            !state.overlays.some(
              (overlay) => overlay.symbol === OVERLAYS.dialog && overlay.open,
            ),
        );
        await toggleShield();
        await g.capture(`${label}-shield-rearmed`);
        await scenario.evidence.record("shield-follows", {
          label,
          button,
          side,
          rays,
        });
      };
      await shieldProbe("flat-exploded");

      // The exploded panel stays usable where it is: a press at REDUCED
      // MOTION's place on the Surface toggles it, and toggles it back.
      const motion = PANEL.reducedMotion;
      const [press] = await projectContent(
        g,
        [[motion[0] + motion[2] / 2, motion[1] + motion[3] / 2]],
        LAYERS.advanced * LAYERS.spacing,
      );
      for (const expected of [true, false]) {
        await g.page.mouse.click(press!.clientX, press!.clientY);
        await waitForGuiState(g, (state) => {
          const value = controlValue(state, "checkbox", "REDUCED MOTION");
          return value.kind === "bool" && value.value === expected;
        });
      }

      // Ordinary camera navigation remains available in the inspection mode.
      const canvasBox = await g.page.locator("#ipp-world-canvas").boundingBox();
      assert.ok(canvasBox);
      await g.page.mouse.move(canvasBox.x + 30, canvasBox.y + 50);
      await g.page.mouse.down();
      await g.page.mouse.move(canvasBox.x + 95, canvasBox.y + 75, { steps: 6 });
      await g.page.mouse.up();
      await g.waitFor((inspection) => {
        const camera = transform(inspection);
        return Math.abs(Number(camera.qy) - Number(inspectionCamera.qy)) > 1e-4;
      });
      await g.page.locator("#reset-camera").click();
      await g.waitFor((inspection) => {
        const camera = transform(inspection);
        return ["x", "y", "z", "qx", "qy", "qz", "qw"].every(
          (field) =>
            Math.abs(Number(camera[field]) - Number(inspectionCamera[field])) <
            1e-5,
        );
      });

      // The committed in-panel switch follows the same camera path as sidebar.
      const [explodePress] = await projectContent(
        g,
        [
          [
            PANEL.explode[0] + PANEL.explode[2] / 2,
            PANEL.explode[1] + PANEL.explode[3] / 2,
          ],
        ],
        LAYERS.advanced * LAYERS.spacing,
      );
      await g.page.mouse.click(explodePress!.clientX, explodePress!.clientY);

      // Flattening closes the planes on the Host clock; the panel paints
      // as it did before.
      await g.waitFor(
        (inspection) =>
          Number(
            fieldsWith(inspection, PANEL_ENTITY, "layer_spacing").layer_spacing,
          ) < 1e-4,
      );
      await g.page.waitForFunction(
        () => document.querySelector("#gui-layers")?.textContent === "flat",
      );
      await g.page.mouse.move(1, 1);
      await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));
      const flattened = await g.capture("layers-flattened");
      const centre = await g.call<{ changedPixels: number }>(
        "compareViewerCaptureRegion",
        "layers-flat",
        "layers-flattened",
        // The centre column: the log beside it records the toggles.
        await g.call("galleryGuiContentRegion", [
          PANEL.monitorPanel[0],
          16,
          PANEL.monitorPanel[0] + PANEL.monitorPanel[2],
          656,
        ]),
      );
      assert.ok(
        centre.changedPixels < 50,
        `the flattened panel does not paint as before: ${JSON.stringify(centre)}`,
      );
      assert.equal(flattened.frame.failedDrawCalls, 0);
      const restoredCamera = transform(flattened.inspection);
      for (const field of ["x", "y", "z", "qx", "qy", "qz", "qw"])
        assert.ok(
          Math.abs(
            Number(restoredCamera[field]) - Number(ordinaryCamera[field]),
          ) < 1e-5,
        );

      // Reduced motion jumps to an inward spherical shell stack; its
      // in-panel controls remain reachable through the curved projection.
      await g.call(
        "galleryGuiAction",
        { role: "checkbox", name: "REDUCED MOTION" },
        { kind: "toggle" },
      );
      await waitForGuiState(g, (state) => {
        const value = controlValue(state, "checkbox", "REDUCED MOTION");
        return value.kind === "bool" && value.value;
      });
      await g.page.locator("#gui-surface-shape").selectOption("sphere");
      await g.page.locator("#gui-surface-facing").selectOption("inside");
      await g.waitFor((inspection) => {
        const provider = sceneEntity(inspection, PANEL_ENTITY).components.find(
          (component) => "layer_spacing" in component.fields,
        );
        return (
          provider?.component === 55 && provider.fields.curvature === -0.125
        );
      });
      await g.capture("layers-inward-flat-ready");
      await g.page.locator("#gui-explode-toggle").click();
      try {
        await g.waitFor(
          (inspection) =>
            Math.abs(
              Number(
                fieldsWith(inspection, PANEL_ENTITY, "layer_spacing")
                  .layer_spacing,
              ) - LAYERS.spacing,
            ) < 1e-4,
        );
      } catch (failure) {
        await g.capture("layers-inward-spacing-failure");
        throw failure;
      }
      const [scopePress] = await projectContent(
        g,
        [
          [
            PANEL.scope[0] + PANEL.scope[2] / 2,
            PANEL.scope[1] + PANEL.scope[3] / 2,
          ],
        ],
        LAYERS.panel * LAYERS.spacing,
      );
      await g.page.mouse.click(scopePress!.clientX, scopePress!.clientY);
      const curvedPopover = await waitForGuiState(g, (state) =>
        state.overlays.some(
          (overlay) => overlay.symbol === OVERLAYS.scope && overlay.open,
        ),
      );
      const curvedOption = control(curvedPopover, {
        role: "button",
        symbol: "gui-scope-grid/scanlines/label",
      });
      const [optionPress] = await projectContent(
        g,
        [
          [
            curvedOption.bounds[0] + curvedOption.bounds[2] * 0.7,
            curvedOption.bounds[1] + curvedOption.bounds[3] / 2,
          ],
        ],
        LAYERS.anchored * LAYERS.spacing,
      );
      assert.ok(
        optionPress!.x > 0.02 &&
          optionPress!.x < 0.98 &&
          optionPress!.y > 0.02 &&
          optionPress!.y < 0.98,
        "the inward popup shell is outside the inspection frame",
      );
      const curved = await g.capture("layers-exploded-inward");
      assert.equal(curved.frame.failedDrawCalls, 0);
      await g.page.screenshot({
        path: resolve(
          scenario.evidence.directory,
          "layers-exploded-inward-page.png",
        ),
        fullPage: true,
      });
      await g.page.mouse.click(optionPress!.clientX, optionPress!.clientY);
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-scope")?.textContent ===
          "scanlines, sweep on",
      );
      // Dismiss the popup without forwarding the closing press to a control.
      const [outsidePopup] = await projectContent(g, [[20, 640]]);
      await g.page.mouse.click(outsidePopup!.clientX, outsidePopup!.clientY);
      await waitForGuiState(
        g,
        (state) =>
          !state.overlays.some(
            (overlay) => overlay.symbol === OVERLAYS.scope && overlay.open,
          ),
      );
      const [curvedMotion] = await projectContent(
        g,
        [[motion[0] + motion[2] / 2, motion[1] + motion[3] / 2]],
        LAYERS.advanced * LAYERS.spacing,
      );
      await g.page.mouse.click(curvedMotion!.clientX, curvedMotion!.clientY);
      await waitForGuiState(g, (state) => {
        const value = controlValue(state, "checkbox", "REDUCED MOTION");
        return value.kind === "bool" && !value.value;
      });
      await shieldProbe("sphere-inside-exploded", false);
      for (const [shape, facing] of [
        ["cylinder", "outside"],
        ["cylinder", "inside"],
        ["sphere", "outside"],
      ] as const) {
        await g.page.locator("#gui-surface-shape").selectOption(shape);
        await g.page.locator("#gui-surface-facing").selectOption(facing);
        await g.waitFor((inspection) => {
          const provider = sceneEntity(
            inspection,
            PANEL_ENTITY,
          ).components.find((component) => "layer_spacing" in component.fields);
          return (
            provider?.component === (shape === "cylinder" ? 54 : 55) &&
            Math.abs(Number(provider.fields.layer_spacing) - LAYERS.spacing) <
              1e-4 &&
            Number(provider.fields.curvature) ===
              (facing === "inside" ? -0.125 : 0.125)
          );
        });
        await shieldProbe(`${shape}-${facing}-exploded`, facing === "outside");
      }
      const priorShield = sceneEntity(await g.inspect(), "gui-input-shield").id;
      await g.page.locator("#gui-vector-only").click();
      await g.waitFor(
        (inspection) =>
          !inspection.entities.some(
            (entity) => entity.metadata.symbolicId === "gui-input-shield",
          ),
      );
      await g.page.locator("#gui-vector-only").click();
      await g.waitFor((inspection) => {
        const shield = inspection.entities.find(
          (entity) => entity.metadata.symbolicId === "gui-input-shield",
        );
        return (
          shield !== undefined &&
          shield.id !== priorShield &&
          Math.abs(
            Number(
              fieldsWith(inspection, PANEL_ENTITY, "layer_spacing")
                .layer_spacing,
            ) - LAYERS.spacing,
          ) < 1e-4
        );
      });
      await shieldProbe("sphere-outside-restored");
      assert.deepEqual(g.errors, []);
    },
  );
});

test("Gallery GUI floats a tooltip, a context menu and a confirmation dialog on the kit's overlay layers", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "GUI overlays",
    {
      ...galleryEnvironment,
      evidenceParent: resolve(
        "target/integration-artifacts/gallery-gui/overlays",
      ),
    },
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario, {
        initialPage: "gui",
        canvasShare: PIXEL_COVERAGE_CANVAS_SHARE,
      });
      await g.page.waitForFunction(
        () =>
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "ready",
      );
      await awaitStationIdle(g);
      // Static frames: SCAN off.
      await g.call(
        "galleryGuiAction",
        { role: "checkbox", name: "SCAN" },
        { kind: "toggle" },
      );
      await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));
      const overlay = (state: GalleryGuiState, symbol: string) =>
        state.overlays.find((candidate) => candidate.symbol === symbol);
      const text = async (selector: string) =>
        g.page.locator(selector).textContent();
      const newestEvent = (state: GalleryGuiState) =>
        state.eventLog.items[0]?.text ?? "";

      // PULSE's tooltip: the runtime opens it after its delay while the
      // pointer hovers PULSE, above it and beyond the monitor's frame, so it
      // paints over the page between the panels, and closes it when the
      // pointer leaves.
      await g.page.mouse.move(1, 1);
      await g.capture("overlays-quiet");
      const pulse = await controlPoint(g, { role: "button", name: "PULSE" });
      await g.page.mouse.move(pulse.clientX, pulse.clientY);
      await waitForGuiState(
        g,
        (state) => overlay(state, OVERLAYS.pulseTip)?.visible === true,
      );
      await g.capture("overlays-tooltip");
      const tip = PANEL.tooltip(PANEL.pulse, "Send a burst across the scope");
      const gap: LogicalRect = [
        PANEL.monitorPanel[0] + PANEL.monitorPanel[2] + 3,
        tip[1] + 3,
        PANEL.nodesPanel[0] - 3,
        tip[1] + tip[3] - 3,
      ];
      const [quietGap, tipGap] = await Promise.all(
        ["overlays-quiet", "overlays-tooltip"].map(
          async (label) =>
            (
              await g.call<Record<string, RegionStats>>(
                "galleryGuiRegionStats",
                label,
                { gap },
              )
            ).gap!,
        ),
      );
      await scenario.evidence.record("tooltip-gap", { quietGap, tipGap });
      assert.ok(
        Math.max(...quietGap!.max) < 90,
        `the page between the panels is not quiet: ${JSON.stringify(quietGap)}`,
      );
      assert.ok(
        Math.max(...tipGap!.max) > 150,
        `the tooltip does not reach beyond the monitor: ${JSON.stringify(tipGap)}`,
      );
      // The pointer moves on to the page between the panels, which ends
      // the hover, and the tooltip closes after its grace.
      const [between] = await projectContent(g, [
        [
          (PANEL.monitorPanel[0] +
            PANEL.monitorPanel[2] +
            PANEL.nodesPanel[0]) /
            2,
          PANEL.statusPanel[1] + 40,
        ],
      ]);
      await g.page.mouse.move(between!.clientX, between!.clientY, {
        steps: 4,
      });
      await waitForGuiState(
        g,
        (state) => overlay(state, OVERLAYS.pulseTip)?.visible === false,
      );

      // A secondary press on a node row is its context request: the runtime
      // focuses the row, and the menu opens at the press with rows that take
      // no focus. Ping logs the node's answer and closes the menu.
      const rowPoint = async (index: number) => {
        const row = PANEL.gridRow(index);
        const [point] = await projectContent(g, [
          [row[0] + row[2] / 2, row[1] + row[3] / 2],
        ]);
        return point!;
      };
      const alpha = await rowPoint(0);
      await g.page.mouse.click(alpha.clientX, alpha.clientY, {
        button: "right",
      });
      const menuOpen = await waitForGuiState(
        g,
        (state) => overlay(state, OVERLAYS.menuSurface)?.visible === true,
      );
      await g.page.mouse.move(1, 1);
      await g.capture("overlays-menu");
      assert.equal(
        control(menuOpen, { role: "button", symbol: "gui-nodes/row/alpha" })
          .focused,
        true,
      );
      assert.deepEqual(
        menuOpen.controls
          .filter(({ symbol }) => symbol?.startsWith(`${OVERLAYS.menu}/`))
          .map(({ label, enabled, focused }) => [label, enabled, focused]),
        [
          ["Rename", true, false],
          ["Ping", true, false],
          ["Remove", true, false],
        ],
      );
      await g.call(
        "galleryGuiAction",
        { role: "button", name: "Ping" },
        { kind: "press" },
      );
      const pinged = await waitForGuiState(
        g,
        (state) =>
          overlay(state, OVERLAYS.menuSurface)?.visible === false &&
          newestEvent(state).endsWith(
            `NODE ALPHA ANSWERED ${nodeSignal("alpha", 0.64)}%`,
          ),
      );
      assert.ok(pinged);
      // Remove takes Bravo, the sixth strongest, out of the table; its toast
      // restores it.
      const bravo = await rowPoint(5);
      await g.page.mouse.click(bravo.clientX, bravo.clientY, {
        button: "right",
      });
      await waitForGuiState(
        g,
        (state) => overlay(state, OVERLAYS.menuSurface)?.visible === true,
      );
      await g.call(
        "galleryGuiAction",
        { role: "button", name: "Remove" },
        { kind: "press" },
      );
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-nodes")?.textContent === "5 of 7 online",
      );
      const removed = await waitForGuiState(g, (state) =>
        state.controls.some(({ label }) => label === "Bravo removed."),
      );
      assert.ok(
        !removed.controls.some(
          ({ symbol }) => symbol === "gui-nodes/row/bravo",
        ),
      );
      await g.call(
        "galleryGuiAction",
        { role: "button", name: "RESTORE" },
        { kind: "press" },
      );
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-nodes")?.textContent === "6 of 8 online",
      );
      await waitForGuiState(
        g,
        (state) =>
          newestEvent(state).endsWith("NODE BRAVO RESTORED") &&
          !state.controls.some(({ symbol }) =>
            /^gui-toasts\/[^/]+$/.test(symbol ?? ""),
          ),
      );

      // Lift the real cover before exercising PURGE's modal interaction.
      await liftInputShield(g);
      // PURGE asks first: a modal dialog on the dialog layer, focus on
      // Cancel. A press beneath it reaches nothing, and Escape cancels and
      // returns focus to PURGE.
      const purge = await controlPoint(g, { role: "button", name: "PURGE" });
      await g.page.mouse.click(purge.clientX, purge.clientY);
      const asked = await waitForGuiState(
        g,
        (state) => overlay(state, OVERLAYS.dialog)?.visible === true,
      );
      assert.equal(
        control(asked, {
          role: "button",
          symbol: `${OVERLAYS.dialog}/cancel`,
        }).focused,
        true,
      );
      const sync = await controlPoint(g, { role: "button", name: "SYNC" });
      await g.page.mouse.click(sync.clientX, sync.clientY);
      await g.settle();
      assert.equal(await text("#gui-operation"), "Node sync: complete");
      assert.equal(
        overlay(await waitForGuiState(g), OVERLAYS.dialog)?.visible,
        true,
        "a press beneath the dialog closed it",
      );
      await g.page.keyboard.press("Escape");
      const cancelled = await waitForGuiState(
        g,
        (state) =>
          overlay(state, OVERLAYS.dialog)?.visible === false &&
          newestEvent(state).endsWith("NODE PURGE CANCELLED"),
      );
      assert.equal(
        control(cancelled, { role: "button", name: "PURGE" }).focused,
        true,
      );
      assert.equal(await text("#gui-nodes"), "6 of 8 online");

      // Asked again, Tab moves focus to the amber Purge and Enter confirms:
      // the table empties and its toast offers RESTORE.
      await g.page.mouse.click(purge.clientX, purge.clientY);
      await waitForGuiState(
        g,
        (state) => overlay(state, OVERLAYS.dialog)?.visible === true,
      );
      await g.page.keyboard.press("Tab");
      await waitForGuiState(
        g,
        (state) =>
          control(state, {
            role: "button",
            symbol: `${OVERLAYS.dialog}/action`,
          }).focused,
      );
      await g.page.keyboard.press("Enter");
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-operation")?.textContent ===
          "Node purge: complete",
      );
      await waitForGuiState(
        g,
        (state) =>
          overlay(state, OVERLAYS.dialog)?.visible === false &&
          state.controls.some(({ label }) => label === "Nodes purged."),
      );
      assert.equal(await text("#gui-nodes"), "0 of 0 online");
      assert.deepEqual(g.errors, []);
    },
  );
});
