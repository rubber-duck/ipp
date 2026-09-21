import { invoke, writeDataUrl, bigintJson } from "./evidence.js";
import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import test from "node:test";
import type { CDPSession, ConsoleMessage, Page } from "playwright";
import {
  assertLoopbackClosed,
  type BrowserBuildConfiguration,
  runBrowserEnvironment,
} from "../browser/environment.js";
import { requireVisible } from "./image-assertions.js";
import type {
  CanvasCaptureReport,
  CanvasLayoutObservation,
  CanvasObservation,
  TransferObservation,
} from "./canvas-fixture.js";

const workspace = resolve(process.cwd());
const render = browserBuild("render");
const overlays = browserBuild("headless");
const fixtureModule = "/target/canvas-build/fixture.js";

test("canvas follows CSS size and display density without replacing its runtime", {
  timeout: 60_000,
}, async (context) => {
  const errors: string[] = [];
  const result = await runBrowserEnvironment(
    "DPI-aware declarative canvas",
    {
      workspace,
      build: render,
      mismatchBuild: overlays,
      deviceScaleFactor: 2,
      operationTimeoutMs: 12_000,
      closeTimeoutMs: 5_000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/render-canvas",
      ),
    },
    context.signal,
    async (scenario) => {
      scenario.page.on("pageerror", (error) => errors.push(error.message));
      scenario.page.on("console", (message) =>
        recordConsoleError(message, errors),
      );
      const moduleUrl = `${scenario.urls.origin}${fixtureModule}`;
      let failure: unknown;
      let transfers: TransferObservation | undefined;
      let metrics: CDPSession | undefined;
      try {
        await invoke(scenario.page, moduleUrl, "mountCanvasApplication", [
          {
            generatedModuleUrl: scenario.urls.generated,
            workerScriptUrl: scenario.urls.workerScript,
            wasmUrl: scenario.urls.wasm,
            timeoutMs: 10_000,
          },
          false,
        ]);
        const canvases = await invoke<readonly CanvasObservation[]>(
          scenario.page,
          moduleUrl,
          "waitForCanvasApplication",
        );
        const initialScene = requireCanvas(canvases, "left");
        assert.deepEqual(initialScene.entityIds, [
          "__fixture-camera",
          "canvas-owned-left",
          "canvas-producer-left",
        ]);

        const initial = await capture(
          scenario.page,
          moduleUrl,
          scenario.evidence.directory,
          "dpi-initial",
          640,
          480,
        );
        assertRenderedCube(initial, "DPR 2 initial canvas");
        const initialLayout = await layout(scenario.page, moduleUrl);
        assertLayout(initialLayout, 320, 240, 2, 640, 480);
        assertIdentity(initialLayout, initialScene);
        const initialTransfers = initialLayout.transfers;

        await invoke(scenario.page, moduleUrl, "setCanvasCssSize", [
          "left",
          400,
          200,
        ]);
        const responsive = await capture(
          scenario.page,
          moduleUrl,
          scenario.evidence.directory,
          "dpi-responsive",
          800,
          400,
        );
        assertRenderedCube(responsive, "canvasProps CSS resize");
        const responsiveLayout = await layout(scenario.page, moduleUrl);
        assertLayout(responsiveLayout, 400, 200, 2, 800, 400);
        assertIdentity(responsiveLayout, initialScene);
        assert.deepEqual(responsiveLayout.transfers, initialTransfers);

        metrics = await scenario.page.context().newCDPSession(scenario.page);
        await setDeviceScaleFactor(scenario.page, metrics, 1);
        const dprChanged = await capture(
          scenario.page,
          moduleUrl,
          scenario.evidence.directory,
          "dpi-dynamic-1x",
          400,
          200,
        );
        assertRenderedCube(dprChanged, "dynamic DPR 1 canvas");
        const dprLayout = await layout(scenario.page, moduleUrl);
        assertLayout(dprLayout, 400, 200, 1, 400, 200);
        assertIdentity(dprLayout, initialScene);
        assert.deepEqual(dprLayout.transfers, initialTransfers);

        // Layout and density changes are valid while GPU work is suspended.
        await invoke(scenario.page, moduleUrl, "setCanvasContextLost", [
          "left",
          true,
        ]);
        await setDeviceScaleFactor(scenario.page, metrics, 2);
        await invoke(scenario.page, moduleUrl, "setCanvasCssSize", [
          "left",
          1_200,
          600,
        ]);
        const duringLoss = await layout(scenario.page, moduleUrl);
        assertIdentity(duringLoss, initialScene);
        assert.deepEqual(duringLoss.transfers, initialTransfers);
        await invoke(scenario.page, moduleUrl, "setCanvasContextLost", [
          "left",
          false,
        ]);
        const capped = await capture(
          scenario.page,
          moduleUrl,
          scenario.evidence.directory,
          "dpi-capped",
          2_048,
          1_024,
        );
        assertRenderedCube(
          capped,
          "proportionally capped canvas after context recovery",
        );
        assert.ok(capped.contextGeneration > dprChanged.contextGeneration);
        const cappedLayout = await layout(scenario.page, moduleUrl);
        assertLayout(cappedLayout, 1_200, 600, 2, 2_048, 1_024);
        assertIdentity(cappedLayout, initialScene);
        assert.deepEqual(cappedLayout.transfers, initialTransfers);
        transfers = cappedLayout.transfers;
      } catch (error) {
        failure = error;
        throw error;
      } finally {
        try {
          await metrics?.detach();
          await invoke(scenario.page, moduleUrl, "closeCanvasApplication");
          await waitForWorkerCount(scenario.page, 0);
        } catch (cleanupError) {
          if (failure !== undefined) {
            throw new AggregateError(
              [failure, cleanupError],
              "DPI scenario and cleanup failed",
            );
          }
          throw cleanupError;
        }
      }
      assert.ok(transfers);
      return { transfers, workers: scenario.page.workers().length };
    },
  );
  assert.equal(result.value.workers, 0);
  assert.equal(result.value.transfers.duplicateTransfers, 0);
  assert.equal(result.value.transfers.dimensionWritesAfterTransfer, 0);
  assert.equal(
    result.value.transfers.calls,
    result.value.transfers.distinctCanvases,
  );
  assert.deepEqual(errors, []);
  await assertLoopbackClosed(result.origin);
});

function browserBuild(name: "render" | "headless"): BrowserBuildConfiguration {
  const directory = resolve(workspace, "target/browser-build", name);
  return {
    name,
    generatedModule: resolve(directory, "generated.js"),
    runtimeWasm: resolve(directory, "runtime.wasm"),
    exportWasm: resolve(directory, "export.wasm"),
    contractArtifact: resolve(directory, "contract.bin"),
  };
}

async function layout(
  page: Page,
  moduleUrl: string,
): Promise<CanvasLayoutObservation> {
  return await invoke(page, moduleUrl, "observeCanvasLayout", ["left"]);
}

function assertLayout(
  layout: CanvasLayoutObservation,
  cssWidth: number,
  cssHeight: number,
  devicePixelRatio: number,
  attributeWidth: number,
  attributeHeight: number,
): void {
  assert.equal(layout.cssWidth, cssWidth);
  assert.equal(layout.cssHeight, cssHeight);
  assert.equal(layout.devicePixelRatio, devicePixelRatio);
  assert.equal(layout.attributeWidth, attributeWidth);
  assert.equal(layout.attributeHeight, attributeHeight);
}

function assertIdentity(
  current: CanvasLayoutObservation,
  initial: CanvasObservation,
): void {
  assert.equal(current.observation.session, initial.session);
  assert.deepEqual(current.observation.entityIds, initial.entityIds);
  assert.deepEqual(
    current.observation.runtimeEntityIds,
    initial.runtimeEntityIds,
  );
  assert.equal(current.observation.ownedExists, true);
  assert.deepEqual(current.observation.errors, []);
}

function requireCanvas(
  observations: readonly CanvasObservation[],
  id: "left" | "right",
): CanvasObservation {
  const observation = observations.find((candidate) => candidate.id === id);
  assert.ok(observation, `missing ${id} canvas observation`);
  return observation;
}

async function capture(
  page: Page,
  moduleUrl: string,
  directory: string,
  label: string,
  width: number,
  height: number,
): Promise<CanvasCaptureReport> {
  const report = await invoke<CanvasCaptureReport>(
    page,
    moduleUrl,
    "captureCanvas",
    ["left", label, width, height],
  );
  const dataUrl = await invoke<string>(
    page,
    moduleUrl,
    "canvasCaptureDataUrl",
    [label],
  );
  await Promise.all([
    // Retain the real GPU readback. A Playwright element screenshot restores its
    // context's initial DPR and would undo the live CDP density change under test.
    writeDataUrl(join(directory, `${label}-capture.png`), dataUrl),
    writeFile(
      join(directory, `${label}-frame.json`),
      `${JSON.stringify(report, bigintJson, 2)}\n`,
    ),
  ]);
  return report;
}

function assertRenderedCube(report: CanvasCaptureReport, label: string): void {
  requireVisible(report.summary, label);
  assert.equal(report.drawCalls, 1);
  assert.equal(report.triangles, 12);
}

async function setDeviceScaleFactor(
  page: Page,
  session: CDPSession,
  factor: number,
): Promise<void> {
  const viewport = page.viewportSize();
  assert.ok(viewport, "DPR emulation requires a fixed browser viewport");
  // CDP changes DPR without notifying resize/media listeners when viewport
  // metrics stay identical. Simulate a display move with a viewport resize;
  // the fixture's fixed CSS canvas size stays unchanged.
  await session.send("Emulation.setDeviceMetricsOverride", {
    width: factor === 1 ? viewport.width - 1 : viewport.width,
    height: viewport.height,
    deviceScaleFactor: factor,
    mobile: false,
  });
  await page.waitForFunction(
    (expected) => window.devicePixelRatio === expected,
    factor,
  );
}

async function waitForWorkerCount(page: Page, expected: number): Promise<void> {
  const deadline = Date.now() + 5_000;
  while (page.workers().length !== expected) {
    if (Date.now() >= deadline) {
      assert.equal(
        page.workers().length,
        expected,
        "owned workers did not stop",
      );
    }
    await new Promise<void>((resolvePromise) => setTimeout(resolvePromise, 20));
  }
}

function recordConsoleError(message: ConsoleMessage, errors: string[]): void {
  if (message.type() === "error") errors.push(message.text());
}
