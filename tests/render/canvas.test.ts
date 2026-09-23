import { createPickingRing } from "../integration/camera-fixtures.js";
import {
  resourceUrlMappings,
  resourceFetchUrl,
} from "../../packages/ipp-client/src/resource-urls.js";
import { workerTransport } from "../../packages/ipp-client/src/worker.js";
import { invoke, writeDataUrl, bigintJson } from "./evidence.js";
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import test from "node:test";
import type { ConsoleMessage, Page } from "playwright";
import {
  assertLoopbackClosed,
  type BrowserBuildConfiguration,
  runBrowserEnvironment,
} from "../browser/environment.js";
import { type ImageDifference, requireVisible } from "./image-assertions.js";
import type {
  CanvasCaptureReport,
  CanvasObservation,
  TransferObservation,
} from "./canvas-fixture.js";

const workspace = resolve(process.cwd());
const render = browserBuild("render");
const overlays = browserBuild("headless");

for (const variant of ["development", "production"] as const) {
  test(`${variant}: nested scenes route to independent canvases and clean up`, {
    timeout: 60_000,
  }, async (context) => {
    const errors: string[] = [];
    const result = await runBrowserEnvironment(
      `${variant} declarative canvas composition`,
      {
        workspace,
        build: render,
        mismatchBuild: overlays,
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
        const moduleUrl = fixtureUrl(scenario.urls.origin, variant);
        const configuration = {
          assetCacheBytes: 0,
          generatedModuleUrl: scenario.urls.generated,
          workerScriptUrl: scenario.urls.workerScript,
          wasmUrl: scenario.urls.wasm,
          timeoutMs: 10_000,
        };
        let failure: unknown;
        try {
          await invoke(scenario.page, moduleUrl, "mountCanvasApplication", [
            configuration,
            variant === "development",
          ]);
          const initial = await invoke<readonly CanvasObservation[]>(
            scenario.page,
            moduleUrl,
            "waitForCanvasApplication",
          );
          assert.equal(initial.length, 2);
          const eviction = await invoke<{ before: bigint; after: bigint }>(
            scenario.page,
            moduleUrl,
            "probeAssetCacheEviction",
          );
          assert.notEqual(
            eviction.after,
            eviction.before,
            "zero-byte browser cache target must evict the unused resource",
          );
          // The fixture checks distinct clients; session numbers are Host-local.
          for (const observation of initial) {
            assert.deepEqual(observation.entityIds, [
              "__fixture-camera",
              `canvas-owned-${observation.id}`,
              `canvas-producer-${observation.id}`,
            ]);
            assert.equal(observation.ownedExists, true);
            assert.equal(observation.producer.baseX, 0);
            assertApprox(observation.producer.effectiveX, -0.55);
            assert.deepEqual(
              observation.producer.baseColor?.map(rounded),
              [0.82, 0.18, 0.12],
            );
            assert.deepEqual(
              observation.producer.effectiveColor?.map(rounded),
              [0.15, 0.82, 0.28],
            );
            assert.equal(observation.width, 320);
            assert.equal(observation.height, 240);
            assert.deepEqual(observation.errors, []);
          }

          const initialFrame = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "left",
            `${variant}-left-initial`,
          );
          requireCube(initialFrame, "initial nested World");

          const updated = await invoke<readonly CanvasObservation[]>(
            scenario.page,
            moduleUrl,
            "updateSharedScene",
            [0.62, 0.42, [0.72, 0.25, 0.9]],
          );
          for (const observation of updated) {
            const prior = initial.find(({ id }) => id === observation.id);
            assert.equal(observation.session, prior?.session);
            assertApprox(observation.producer.effectiveX, 0.62);
            assert.deepEqual(
              observation.producer.effectiveColor?.map(rounded),
              [0.72, 0.25, 0.9],
            );
          }
          const updatedFrame = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "left",
            `${variant}-left-updated`,
          );
          requireCube(updatedFrame, "context-updated World");
          const difference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareCanvasCaptures",
            [`${variant}-left-initial`, `${variant}-left-updated`],
          );
          assert.ok(difference.changedFraction > 0.01);

          const callbacks = await invoke<{
            readonly before: Readonly<Record<string, number>>;
            readonly after: Readonly<Record<string, number>>;
          }>(scenario.page, moduleUrl, "replaceCommitCallbacks");
          assert.deepEqual(callbacks.after, callbacks.before);

          const resized = await invoke<{
            readonly session: bigint;
            readonly frameWidth: number;
            readonly frameHeight: number;
            readonly domWidth: number;
            readonly domHeight: number;
          }>(scenario.page, moduleUrl, "resizeCanvas", ["right", 256, 192]);
          assert.equal(resized.session, initial[1]?.session);
          assert.equal(resized.frameWidth, 256);
          assert.equal(resized.frameHeight, 192);
          assert.equal(resized.domWidth, 256);
          assert.equal(resized.domHeight, 192);

          if (variant === "development") {
            const rejected = await invoke<{
              readonly error: string;
              readonly observation: CanvasObservation;
            }>(scenario.page, moduleUrl, "rejectSceneUpdate", ["left"]);
            assert.match(rejected.error, /nonfinite|finite/i);
            assertApprox(rejected.observation.producer.effectiveX, 0.62);
            const recovered = await invoke<CanvasObservation>(
              scenario.page,
              moduleUrl,
              "recoverSceneUpdate",
              ["left"],
            );
            assertApprox(recovered.producer.effectiveX, 0.62);
            const callbackFailure = await invoke<{
              readonly error: string;
              readonly duringFailure: CanvasObservation;
              readonly recovered: CanvasObservation;
            }>(scenario.page, moduleUrl, "rejectAsyncCommit", ["left"]);
            assert.match(
              callbackFailure.error,
              /async onCommit failed for left/,
            );
            assertApprox(
              callbackFailure.duringFailure.producer.effectiveX,
              0.72,
            );
            assertApprox(callbackFailure.recovered.producer.effectiveX, 0.82);
          }

          const replacement = await invoke<{
            readonly oldSession: bigint;
            readonly replacement: CanvasObservation;
            readonly other: CanvasObservation;
            readonly transfers: TransferObservation;
          }>(scenario.page, moduleUrl, "replaceCanvasRuntime", ["left"]);
          assert.equal(replacement.oldSession, initial[0]?.session);
          assert.equal(replacement.other.session, initial[1]?.session);
          assert.equal(replacement.replacement.ownedExists, true);
          assert.deepEqual(replacement.replacement.entityIds, [
            "__fixture-camera",
            "canvas-owned-left",
            "canvas-producer-left",
          ]);
          assertApprox(
            replacement.replacement.producer.effectiveX,
            variant === "development" ? 0.82 : 0.62,
          );
          assertApprox(
            replacement.other.producer.effectiveX,
            variant === "development" ? 0.82 : 0.62,
          );
          assert.equal(replacement.transfers.duplicateTransfers, 0);
          assert.equal(
            replacement.transfers.calls,
            replacement.transfers.distinctCanvases,
          );
          const replacementFrame = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "left",
            `${variant}-left-runtime-replacement`,
          );
          requireCube(replacementFrame, "replacement canvas World");
          assert.equal(
            replacementFrame.session,
            replacement.replacement.session,
          );

          const removed = await invoke<{
            readonly removed: CanvasObservation;
            readonly other: CanvasObservation;
          }>(scenario.page, moduleUrl, "removeScene", ["left"]);
          assert.equal(removed.removed.ownedExists, false);
          assert.equal(removed.removed.producer.baseX, 0);
          assert.equal(removed.removed.producer.effectiveX, 0);
          assert.equal(removed.other.ownedExists, true);
          assertApprox(
            removed.other.producer.effectiveX,
            variant === "development" ? 0.82 : 0.62,
          );
          const producerFrame = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "left",
            `${variant}-left-producer-after-scene`,
          );
          requireCube(producerFrame, "producer after World cleanup");

          const workerCount = scenario.page.workers().length;
          const canvasRemoval = await invoke<{
            readonly closedSession: bigint;
            readonly remaining: CanvasObservation;
          }>(scenario.page, moduleUrl, "removeCanvas", ["left"]);
          assert.equal(
            canvasRemoval.closedSession,
            replacement.replacement.session,
          );
          assert.equal(canvasRemoval.remaining.session, initial[1]?.session);
          await waitForWorkerCount(scenario.page, workerCount - 1);
          const transfers = await invoke<TransferObservation>(
            scenario.page,
            moduleUrl,
            "transferObservation",
          );
          assert.equal(transfers.duplicateTransfers, 0);
          assert.equal(transfers.calls, transfers.distinctCanvases);
          assert.ok(transfers.calls >= 2);
          return { transfers, workers: scenario.page.workers().length };
        } catch (error) {
          failure = error;
          throw error;
        } finally {
          try {
            await invoke(scenario.page, moduleUrl, "closeCanvasApplication");
            await waitForWorkerCount(scenario.page, 0);
          } catch (cleanupError) {
            if (failure !== undefined) {
              throw new AggregateError(
                [failure, cleanupError],
                "canvas scenario and cleanup failed",
              );
            }
            throw cleanupError;
          }
        }
      },
    );
    assert.equal(result.value.workers, 1);
    assert.deepEqual(errors, []);
    await assertLoopbackClosed(result.origin);
  });
}

test("StrictMode unmount aborts a real worker during gated WASM startup", {
  timeout: 30_000,
}, async (context) => {
  const errors: string[] = [];
  const result = await runBrowserEnvironment(
    "StrictMode pending canvas startup",
    {
      workspace,
      build: render,
      mismatchBuild: overlays,
      operationTimeoutMs: 12_000,
      closeTimeoutMs: 5_000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/render-canvas",
      ),
    },
    context.signal,
    async (scenario) => {
      const moduleUrl = fixtureUrl(scenario.urls.origin, "development");
      let releaseRoute = (): void => undefined;
      let markIntercepted = (): void => undefined;
      const routeGate = new Promise<void>((resolvePromise) => {
        releaseRoute = resolvePromise;
      });
      const intercepted = new Promise<void>((resolvePromise) => {
        markIntercepted = resolvePromise;
      });
      scenario.page.on("pageerror", (error) => errors.push(error.message));
      scenario.page.on("console", (message) =>
        recordConsoleError(message, errors),
      );
      await scenario.page.route(scenario.urls.wasm, async (route) => {
        markIntercepted();
        await routeGate;
        await route.continue().catch(() => undefined);
      });
      let failure: unknown;
      try {
        await invoke(scenario.page, moduleUrl, "mountPendingCanvas", [
          {
            generatedModuleUrl: scenario.urls.generated,
            workerScriptUrl: scenario.urls.workerScript,
            wasmUrl: scenario.urls.wasm,
            timeoutMs: 10_000,
          },
        ]);
        await withTimeout(intercepted, 5_000, "real worker WASM request");
        await waitForWorkerCount(scenario.page, 1);
        const duringStartup = await invoke<{
          readonly readyCount: number;
          readonly errors: readonly string[];
          readonly canvasPresent: boolean;
          readonly transfers: TransferObservation;
        }>(scenario.page, moduleUrl, "pendingCanvasObservation");
        assert.equal(duringStartup.readyCount, 0);
        assert.equal(duringStartup.canvasPresent, true);
        assert.deepEqual(duringStartup.errors, []);
        assert.equal(duringStartup.transfers.duplicateTransfers, 0);
        assert.equal(duringStartup.transfers.calls, 1);

        await invoke(scenario.page, moduleUrl, "closePendingCanvas");
        await waitForWorkerCount(scenario.page, 0);
        releaseRoute();
        await scenario.page.unroute(scenario.urls.wasm);
        const afterUnmount = await invoke<{
          readonly readyCount: number;
          readonly errors: readonly string[];
          readonly canvasPresent: boolean;
        }>(scenario.page, moduleUrl, "pendingCanvasObservation");
        assert.equal(afterUnmount.readyCount, 0);
        assert.equal(afterUnmount.canvasPresent, false);
        assert.deepEqual(afterUnmount.errors, []);
        return duringStartup.transfers;
      } catch (error) {
        failure = error;
        throw error;
      } finally {
        releaseRoute();
        await scenario.page.unroute(scenario.urls.wasm).catch(() => undefined);
        try {
          await invoke(scenario.page, moduleUrl, "closePendingCanvas");
          await waitForWorkerCount(scenario.page, 0);
        } catch (cleanupError) {
          if (failure !== undefined) {
            throw new AggregateError(
              [failure, cleanupError],
              "pending canvas scenario and cleanup failed",
            );
          }
          throw cleanupError;
        }
      }
    },
  );
  assert.equal(result.value.calls, result.value.distinctCanvases);
  assert.deepEqual(errors, []);
  await assertLoopbackClosed(result.origin);
});

for (const variant of ["development", "production"] as const) {
  test(`${variant}: saved World startup initializes before bindings and owns URL replacement`, {
    timeout: 60000,
  }, async (context) => {
    await runBrowserEnvironment(
      `saved World canvas ${variant}`,
      {
        workspace,
        build: render,
        mismatchBuild: overlays,
        operationTimeoutMs: 15000,
        closeTimeoutMs: 5000,
        evidenceParent: resolve(
          workspace,
          "target/integration-artifacts/render-canvas",
        ),
      },
      context.signal,
      async (scenario) => {
        const module = fixtureUrl(scenario.urls.origin, variant);
        const config = {
          generatedModuleUrl: scenario.urls.generated,
          workerScriptUrl: scenario.urls.workerScript,
          wasmUrl: scenario.urls.wasm,
        };
        const call = <T>(name: string, args: unknown[] = []) =>
          invoke<T>(scenario.page, module, name, args);
        const bytes = await call<number[]>("makeSavedCanvasWorld", [config]);
        await waitForWorkerCount(scenario.page, 0);
        const file = join(scenario.evidence.directory, "saved.world");
        await writeFile(file, new Uint8Array(bytes));
        const url = `${scenario.urls.origin}/${file.slice(workspace.length + 1)}`;
        type Observation = Awaited<
          ReturnType<
            typeof import("./canvas-fixture.js").savedCanvasObservation
          >
        >;
        try {
          await call("mountSavedCanvas", [
            config,
            url,
            "gate",
            variant === "development",
          ]);
          const pending = await call<Observation>("waitForSavedCanvas", [
            "initialized",
          ]);
          assert.equal(pending.ready, 0);
          assert.equal(pending.commits, 0);
          assert.equal(pending.finished, 0);
          await call("releaseSavedInitialization");
          const ready = await call<Observation>("waitForSavedCanvas", [
            "ready",
          ]);
          assert.equal(ready.initialized, 1);
          assert.equal(ready.finished, 1);
          assert.equal(ready.ready, 1);
          assert.deepEqual(ready.errors, []);
          assert.deepEqual(ready.entities.sort(), [
            "__fixture-camera",
            "saved-cube",
          ]);
          const image =
            await call<
              Awaited<
                ReturnType<
                  typeof import("./canvas-fixture.js").captureSavedCanvas
                >
              >
            >("captureSavedCanvas");
          requireVisible(image.summary, "initialized saved World");
          assert.equal(image.drawCalls, 1);
          assert.equal(image.triangles, 12);
          await writeDataUrl(
            join(scenario.evidence.directory, "saved-world.png"),
            image.dataUrl,
          );
          const rerendered = await call<Observation>(
            "rerenderSavedInitializer",
          );
          assert.equal(
            rerendered.initialized,
            1,
            "Callback identity does not restart the World",
          );
          // Replacing the URL starts a fresh saved World and closes the prior worker.
          await call("replaceSavedWorld", [`${url}?version=2`]);
          await call("waitForSavedCanvas", ["initialized", 2]);
          await call("releaseSavedInitialization");
          const replaced = await call<Observation>("waitForSavedCanvas", [
            "ready",
            2,
          ]);
          assert.equal(replaced.initialized, 2);
          assert.equal(replaced.ready, 2);
          assert.deepEqual(replaced.errors, []);
          assert.equal(replaced.transfers.duplicateTransfers, 0);
          await waitForWorkerCount(scenario.page, 1);
          await call("closeSavedCanvas");
          await waitForWorkerCount(scenario.page, 0);

          await call("mountSavedCanvas", [
            config,
            url,
            "throw",
            variant === "development",
          ]);
          const rejected = await call<Observation>("waitForSavedCanvas", [
            "failed",
          ]);
          assert.match(rejected.errors[0]!, /initializer rejected/);
          const resizedFailure = await call<Observation>(
            "resizeFailedSavedCanvas",
          );
          assert.deepEqual(resizedFailure.errors, rejected.errors);
          assert.equal(resizedFailure.ready, 0);
          assert.equal(rejected.ready, 0);
          assert.equal(rejected.commits, 0);
          await waitForWorkerCount(scenario.page, 0);
          await call("closeSavedCanvas");

          await call("mountSavedCanvas", [
            config,
            url,
            "gate",
            variant === "development",
          ]);
          await call("waitForSavedCanvas", ["initialized"]);
          await call("unmountSavedCanvas");
          await waitForWorkerCount(scenario.page, 0);
          await call("releaseSavedInitialization");
          const cancelled = await call<Observation>("savedCanvasObservation");
          assert.equal(cancelled.aborted, 1);
          assert.equal(cancelled.ready, 0);
          assert.equal(cancelled.commits, 0);
          assert.deepEqual(cancelled.errors, []);
        } finally {
          await call("closeSavedCanvas");
          await waitForWorkerCount(scenario.page, 0);
        }
      },
    );
  });
}

test("saved World HTTP fetch aborts and closes its already connected Host", {
  timeout: 30000,
}, async (context) => {
  await runBrowserEnvironment(
    "cancel saved World fetch",
    {
      workspace,
      build: render,
      mismatchBuild: overlays,
      operationTimeoutMs: 12000,
      closeTimeoutMs: 5000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/render-canvas",
      ),
    },
    context.signal,
    async (scenario) => {
      const module = fixtureUrl(scenario.urls.origin, "development");
      const config = {
        generatedModuleUrl: scenario.urls.generated,
        workerScriptUrl: scenario.urls.workerScript,
        wasmUrl: scenario.urls.wasm,
      };
      const url = `${scenario.urls.origin}/pending.world`;
      let release!: () => void;
      let reached!: () => void;
      const pending = new Promise<void>((resolve) => {
        reached = resolve;
      });
      const gate = new Promise<void>((resolve) => {
        release = resolve;
      });
      await scenario.page.route(url, async (route) => {
        reached();
        await gate;
        await route.abort().catch(() => {});
      });
      try {
        await invoke(scenario.page, module, "mountSavedCanvas", [config, url]);
        await withTimeout(pending, 10000, "World fetch");
        await waitForWorkerCount(scenario.page, 1);
        await invoke(scenario.page, module, "unmountSavedCanvas");
        await waitForWorkerCount(scenario.page, 0);
        const observation = await invoke<
          Awaited<
            ReturnType<
              typeof import("./canvas-fixture.js").savedCanvasObservation
            >
          >
        >(scenario.page, module, "savedCanvasObservation");
        assert.equal(observation.ready, 0);
        assert.equal(observation.initialized, 0);
        assert.deepEqual(observation.errors, []);
      } finally {
        release();
        await scenario.page.unroute(url);
        await invoke(scenario.page, module, "closeSavedCanvas");
        await waitForWorkerCount(scenario.page, 0);
      }
    },
  );
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

function fixtureUrl(
  origin: string,
  variant: "development" | "production",
): string {
  return `${origin}/target/canvas-build/${variant === "production" ? "fixture-production.js" : "fixture.js"}`;
}

async function capture(
  page: Page,
  moduleUrl: string,
  directory: string,
  id: "left" | "right",
  label: string,
): Promise<CanvasCaptureReport> {
  const report = await invoke<CanvasCaptureReport>(
    page,
    moduleUrl,
    "captureCanvas",
    [id, label],
  );
  const dataUrl = await invoke<string>(
    page,
    moduleUrl,
    "canvasCaptureDataUrl",
    [label],
  );
  await Promise.all([
    writeDataUrl(join(directory, `${label}-capture.png`), dataUrl),
    writeFile(
      join(directory, `${label}-frame.json`),
      `${JSON.stringify(report, bigintJson, 2)}\n`,
    ),
    page.locator(`#canvas-${id}`).screenshot({
      path: join(directory, `${label}-canvas.png`),
    }),
  ]);
  return report;
}

function requireCube(report: CanvasCaptureReport, label: string): void {
  requireVisible(report.summary, label);
  assert.equal(report.drawCalls, 1);
  assert.equal(report.triangles, 12);
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

async function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  label: string,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(
          () => reject(new Error(`Timed out waiting for ${label}`)),
          timeoutMs,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

function recordConsoleError(message: ConsoleMessage, errors: string[]): void {
  if (message.type() === "error") errors.push(message.text());
}

function assertApprox(actual: number | null, expected: number): void {
  assert.notEqual(actual, null);
  assert.ok(Math.abs((actual ?? 0) - expected) < 0.001);
}

function rounded(value: number): number {
  return Math.round(value * 100) / 100;
}

test("resource URL mapping validates directories before worker creation and preserves suffixes", () => {
  const mapping = {
    prefix: "https://assets.ipp.invalid/",
    baseUrl: "http://localhost:1234/local/",
  };
  const mappings = resourceUrlMappings([mapping]);
  assert.equal(
    resourceFetchUrl(
      new URL("https://assets.ipp.invalid/nested/mesh.bin?version=2&name=%2F"),
      mappings,
    ),
    "http://localhost:1234/local/nested/mesh.bin?version=2&name=%2F",
  );
  assert.equal(
    resourceFetchUrl(new URL("https://other.invalid/mesh.bin"), mappings),
    "https://other.invalid/mesh.bin",
  );
  for (const invalid of [
    [{ ...mapping, prefix: "file:///assets/" }],
    [{ ...mapping, baseUrl: "/relative/" }],
    [{ ...mapping, baseUrl: "https://local.invalid/file" }],
    [{ ...mapping, baseUrl: "https://local.invalid/?query=1" }],
    [mapping, { ...mapping, prefix: `${mapping.prefix}nested/` }],
    [mapping, mapping],
  ])
    assert.throws(
      () =>
        workerTransport("worker.js", "runtime.wasm", 1_048_576, {
          resourceUrls: invalid,
        }),
      TypeError,
    );
});

test("saved World HTTP assets fetch through connection mappings without changing source identity", {
  timeout: 30000,
}, async (context) => {
  await runBrowserEnvironment(
    "saved World resource URL mapping",
    {
      workspace,
      build: render,
      mismatchBuild: overlays,
      operationTimeoutMs: 15000,
      closeTimeoutMs: 5000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/render-canvas",
      ),
    },
    context.signal,
    async (scenario) => {
      const module = fixtureUrl(scenario.urls.origin, "development");
      const config = {
        generatedModuleUrl: scenario.urls.generated,
        workerScriptUrl: scenario.urls.workerScript,
        wasmUrl: scenario.urls.wasm,
      };
      const source = "https://assets.ipp.invalid/ring.mesh?version=2";
      const call = <T>(name: string, args: unknown[] = []) =>
        invoke<T>(scenario.page, module, name, args);
      const bytes = await call<number[]>("makeSavedCanvasWorld", [
        config,
        source,
      ]);
      await waitForWorkerCount(scenario.page, 0);
      const directory = scenario.evidence.directory;
      await writeFile(
        join(directory, "ring.mesh"),
        new Uint8Array(createPickingRing()),
      );
      await writeFile(join(directory, "saved.world"), new Uint8Array(bytes));
      const baseUrl = `${scenario.urls.origin}/${directory.slice(workspace.length + 1)}/`;
      const requests: string[] = [];
      scenario.page.on("request", (request) => requests.push(request.url()));
      try {
        await call("mountSavedCanvas", [
          {
            ...config,
            resourceUrls: [{ prefix: "https://assets.ipp.invalid/", baseUrl }],
          },
          `${baseUrl}saved.world`,
          "mapped",
          true,
        ]);
        await call("waitForSavedCanvas", ["ready"]);
        const frame = await call<
          Awaited<
            ReturnType<typeof import("./canvas-fixture.js").captureSavedCanvas>
          >
        >("captureSavedCanvas", [source]);
        requireVisible(frame.summary, "mapped saved World asset");
        assert.equal(frame.drawCalls, 1);
        assert.equal(frame.triangles, 8);
        await writeDataUrl(join(directory, "mapped-asset.png"), frame.dataUrl);
        const observed = await call<
          Awaited<
            ReturnType<
              typeof import("./canvas-fixture.js").savedCanvasObservation
            >
          >
        >("savedCanvasObservation");
        assert.deepEqual(observed.errors, []);
        assert.ok(
          observed.resources.some(
            (resource) =>
              resource.source === source && resource.status === "loaded",
          ),
        );
        assert.ok(requests.includes(`${baseUrl}ring.mesh?version=2`));
        assert.equal(
          requests.some((url) => url.startsWith("https://assets.ipp.invalid/")),
          false,
        );
        const same = await call<typeof observed>("replaceSavedResourceUrls", [
          [{ prefix: "https://assets.ipp.invalid/", baseUrl }],
        ]);
        assert.equal(
          same.initialized,
          1,
          "Equal mapping values retain the canvas session",
        );
        await mkdir(join(directory, "alternate"));
        await writeFile(
          join(directory, "alternate/ring.mesh"),
          new Uint8Array(createPickingRing()),
        );
        const changed = await call<typeof observed>(
          "replaceSavedResourceUrls",
          [
            [
              {
                prefix: "https://assets.ipp.invalid/",
                baseUrl: `${baseUrl}alternate/`,
              },
            ],
          ],
        );
        assert.equal(changed.initialized, 2);
        assert.equal(changed.ready, 2);
        await call("captureSavedCanvas", [source]);
        assert.ok(requests.includes(`${baseUrl}alternate/ring.mesh?version=2`));
        assert.deepEqual(changed.errors, []);
        await waitForWorkerCount(scenario.page, 1);
      } finally {
        await call("closeSavedCanvas");
        await waitForWorkerCount(scenario.page, 0);
      }
    },
  );
});
