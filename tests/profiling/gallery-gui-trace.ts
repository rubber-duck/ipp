/** Bounded semantic gallery diagnostics; ordinary timing uses separate commands. */
import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import type { ProfileCapture } from "../../packages/ipp-client/src/profiling.js";
import { hostProfiling } from "../../packages/ipp-client/src/profiling.js";
import { exportProfileTrace } from "../../tools/performance/profile-trace.mjs";
import type { HostContract, HostState } from "../../tools/shared-host/host.js";
import { runBrowserEnvironment } from "../harness/browser.js";
import { runNativeEnvironment } from "../harness/native.js";
import {
  galleryEnvironment,
  openGallery,
} from "../gallery/drivers/browser-gallery.js";
import { controlPoint, gainFraction } from "../gallery/gallery-gui-oracle.js";
import { awaitStationIdle } from "../gallery/support/gallery-gui.js";
import { NativeGalleryDriver } from "../gallery/drivers/native-gallery.js";
import { optionalWorkerCpu } from "./support/worker-profiling.js";
import type { GalleryGuiState } from "../gallery/pages/viewer.js";

const { values } = parseArgs({
  options: {
    backend: { type: "string" },
    cdp: { type: "boolean" },
    output: { type: "string" },
    "max-events": { type: "string" },
    "max-artifact-bytes": { type: "string" },
  },
});
const backend = values.backend;
const output = resolve(values.output ?? "target/performance/gallery-gui-trace");
const maxEvents = Number(values["max-events"]);
const maxArtifactBytes = Number(values["max-artifact-bytes"] ?? 16_777_216);
assert.ok(Number.isSafeInteger(maxArtifactBytes) && maxArtifactBytes > 0);
assert.ok(["browser", "native"].includes(backend ?? ""));
assert.ok(
  Number.isInteger(maxEvents) && maxEvents > 0 && maxEvents <= 0xffffffff,
);
await mkdir(output, { recursive: true });
const controller = new AbortController();
const timer = setTimeout(
  () => controller.abort(new Error("Gallery trace timed out")),
  180_000,
);
let cdpTrack: {
  availability: string;
  reason?: string;
  participant?: string;
  alignment?: string;
} | null = null;
const save = async (
  capture: ProfileCapture,
  evidence: unknown,
  referenceParticipant: {
    context: string;
    timeOrigin: number;
    processId?: number;
  },
) => {
  const trace = exportProfileTrace(capture, {
    referenceParticipant,
    browserTracks: cdpTrack,
  });
  assert.ok(trace.traceEvents.length > 0, "capture contains real CPU spans");
  assert.ok(
    trace.traceEvents.some((event) => event.args.scope === "world"),
    "World spans retained",
  );
  await Promise.all([
    writeFile(
      join(output, "profile-capture.json"),
      JSON.stringify(capture, null, 2),
    ),
    writeFile(join(output, "trace.json"), JSON.stringify(trace)),
    writeFile(
      join(output, "manifest.json"),
      JSON.stringify(
        {
          backend,
          maxEvents,
          maxArtifactBytes,
          evidence,
          traceEvents: trace.traceEvents.length,
          clock: trace.metadata.clock,
          overflow: trace.metadata.overflow,
          tracks: trace.metadata.tracks,
          pipelineRun: process.env.IPP_PIPELINE_RUN ?? null,
        },
        null,
        2,
      ),
    ),
  ]);
};
try {
  if (backend === "browser") {
    await runBrowserEnvironment(
      "gallery semantic trace",
      {
        ...galleryEnvironment,
        build: {
          ...galleryEnvironment.build,
          name: "render-instrumentation",
          generatedModule: resolve(
            "target/browser-build/render-instrumentation/generated.js",
          ),
          runtimeWasm: resolve(
            "target/browser-build/render-instrumentation/runtime.wasm",
          ),
          contractArtifact: resolve(
            "target/browser-build/render-instrumentation/contract.bin",
          ),
        },
        operationTimeoutMs: 60_000,
        evidenceParent: output,
      },
      controller.signal,
      async (scenario) => {
        await scenario.page.setViewportSize({ width: 1280, height: 800 });
        const g = await openGallery(scenario, {
          initialPage: "gui",
          canvasShare: 1,
          entryPath: "/target/gallery-trace/index.html",
          helperPath: "/target/gallery-trace/viewer-browser-helper.js",
        });
        await awaitStationIdle(g);
        const before = await g.capture("trace-before");
        assert.ok(
          before.summary.foregroundPixels > 1000,
          "gallery painted before capture",
        );
        const initial = await g.call<GalleryGuiState>("galleryGuiState");
        const slider = initial.controls.find(
          (control) => control.kind === "slider",
        );
        assert.ok(slider?.value.kind === "scalar");
        let active = false;
        try {
          const from = await controlPoint(
            g,
            { role: "slider" },
            gainFraction(slider.value.value),
          );
          const to = await controlPoint(
            g,
            { role: "slider" },
            gainFraction(0.18),
          );
          await g.call("startGalleryTrace", maxEvents, maxArtifactBytes);
          active = true;
          const drag = async () =>
            g.drag([from.clientX, from.clientY], [to.clientX, to.clientY]);
          if (values.cdp) {
            const sampled = await optionalWorkerCpu(
              scenario.page.context().browser()!,
              scenario.page.workers().at(-1)!.url(),
              drag,
            );
            cdpTrack = sampled.track;
            await writeFile(
              join(output, "worker-cpu-profile.json"),
              JSON.stringify(sampled.track),
            );
          } else await drag();
          await g.settle();
          const changed = await g.call<GalleryGuiState>("galleryGuiState");
          const value = changed.controls.find(
            (control) => control.kind === "slider",
          )?.value;
          assert.ok(
            value?.kind === "scalar" &&
              Math.abs(value.value - slider.value.value) > 0.1,
            "physical drag changes GAIN",
          );
          // Full profiles bypass the capped operation event log; raw arrays live in JSON artifacts.
          const capture = (await scenario.page.evaluate(
            async ({ module }) => (await import(module)).stopGalleryTrace(),
            { module: g.helper },
          )) as ProfileCapture;
          await writeFile(
            join(output, "profile-capture.json"),
            JSON.stringify(capture, null, 2),
          );
          await g.capture("trace-after");
          assert.ok(
            (await g.difference("trace-before", "trace-after")).changedPixels >
              100,
            "drag changes completed image",
          );
          await save(
            capture,
            {
              directory: scenario.evidence.directory,
              initialGain: slider.value.value,
              finalGain: value.value,
              browserVersion: scenario.page.context().browser()?.version(),
            },
            {
              context: `viewer-page:${scenario.page.url()}`,
              timeOrigin: await scenario.page.evaluate(
                () => performance.timeOrigin,
              ),
            },
          );
        } finally {
          if (active) await g.call("cancelGalleryTrace");
        }
      },
    );
  } else {
    const product = resolve("target/gles-host-instrumentation");
    const egl = process.env.IPP_EGL_LIBRARY_DIR;
    assert.ok(egl, "native trace needs EGL directory");
    const ioRead = [
      { prefix: "ipp-gallery://assets/", directory: resolve("target") },
    ];
    await runNativeEnvironment(
      "native gallery semantic trace",
      {
        executable: join(product, "gles_host"),
        schemaArtifact: join(product, "contract.bin"),
        workingDirectory: process.cwd(),
        readinessTimeoutMs: 30_000,
        operationTimeoutMs: 120_000,
        evidenceParent: output,
        extraArguments: [
          "--egl-dir",
          egl,
          "--io-read",
          ioRead[0]!.prefix,
          ioRead[0]!.directory,
        ],
      },
      controller.signal,
      async (environment) => {
        const state = join(environment.evidence.directory, "shared-host");
        await mkdir(state);
        const contract = (await import(
          pathToFileURL(join(product, "generated.js")).href
        )) as HostContract;
        const host: HostState = {
          pid: process.pid,
          url: environment.url,
          presentationUrl: environment.presentationUrl!,
          worktree: process.cwd(),
          commit: "diagnostic-capture",
          contract: contract.SCHEMA_HASH.toString(16),
          client: product,
          font: resolve("target/font-assets/shure-tech-mono.ippf"),
          eglDirectory: egl,
          log: join(environment.evidence.directory, "server-stderr.log"),
          startedAt: new Date().toISOString(),
          ioRead,
        };
        await writeFile(join(state, "host.json"), JSON.stringify(host));
        const observer = await environment.track(
          contract.IppHostClient.connectWebSocket(environment.url),
        );
        const reader = hostProfiling(observer);
        const driver = new NativeGalleryDriver(
          process.cwd(),
          state,
          "trace-gallery-gui",
        );
        let captureId: string | undefined;
        let scenarioFailure: unknown;
        try {
          await driver.start(
            "gui",
            [
              "--width",
              "960",
              "--height",
              "640",
              "--options",
              '{"autoscan":false}',
            ],
            environment.signal,
          );
          const before = await driver.capture(
            join(output, "native-before"),
            environment.signal,
          );
          captureId = await reader.start({
            counters: false,
            trace: { maxEvents },
            maxArtifactBytes,
          });
          await driver.call("options", ['{"gain":0.18}'], environment.signal);
          const after = await driver.capture(
            join(output, "native-after"),
            environment.signal,
          );
          const capture = await reader.stop();
          await writeFile(
            join(output, "profile-capture.json"),
            JSON.stringify(capture, null, 2),
          );
          let painted = 0,
            changed = 0;
          for (let i = 0; i < before.pixels.length; i += 4) {
            if (
              [0, 1, 2].some(
                (axis) =>
                  Math.abs(before.pixels[i + axis]! - before.pixels[axis]!) >
                  20,
              )
            )
              painted++;
            if (
              [0, 1, 2].some(
                (axis) =>
                  Math.abs(after.pixels[i + axis]! - before.pixels[i + axis]!) >
                  20,
              )
            )
              changed++;
          }
          assert.ok(
            painted > 1000 && changed > 100,
            "native gallery renders and changes completed image",
          );
          const inspected = await driver.call(
            "inspect",
            [],
            environment.signal,
          );
          await save(
            capture,
            {
              directory: environment.evidence.directory,
              painted,
              changed,
              state: inspected.report,
              nativeAction: "public gallery gain option",
              fixtureInputs: JSON.parse(
                await readFile(
                  "target/gallery-trace/fixture-inputs.json",
                  "utf8",
                ),
              ),
            },
            {
              context: "native-trace-runner",
              timeOrigin: performance.timeOrigin,
              processId: process.pid,
            },
          );
        } catch (error) {
          scenarioFailure = error;
          throw error;
        } finally {
          const cleanupErrors: unknown[] = [];
          try {
            if (captureId) await reader.release(captureId);
          } catch (error) {
            cleanupErrors.push(error);
          }
          try {
            await driver.close();
          } catch (error) {
            cleanupErrors.push(error);
          }
          if (cleanupErrors.length)
            throw new AggregateError(
              scenarioFailure
                ? [scenarioFailure, ...cleanupErrors]
                : cleanupErrors,
              "Native trace release/session cleanup failed",
            );
        }
        assert.equal(
          (await observer.listWorlds()).length,
          0,
          "gallery Worlds cleaned",
        );
      },
    );
  }
} catch (error) {
  await writeFile(
    join(output, "failure.json"),
    JSON.stringify(
      {
        backend,
        maxEvents,
        maxArtifactBytes,
        message: error instanceof Error ? error.message : String(error),
      },
      null,
      2,
    ),
  );
  throw error;
} finally {
  clearTimeout(timer);
}
