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
  applicationEnvironment,
  openApplication,
} from "../gallery/drivers/browser-application.js";
import { NativeGalleryDriver } from "../gallery/drivers/native-gallery.js";
import { optionalWorkerCpu } from "./support/worker-profiling.js";

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
        ...applicationEnvironment,
        build: {
          ...applicationEnvironment.build,
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
        const g = await openApplication(scenario, {
          entryPath: "/target/gallery-trace/index.html",
          helperPath: "/target/gallery-trace/application-browser-helper.js",
        });
        const before = await g.capture("trace-before");
        assert.ok(before.summary.foregroundPixels > 1000);
        let captureId: string | undefined;
        try {
          captureId = await g.call<string>(
            "startTrace",
            maxEvents,
            maxArtifactBytes,
          );
          const change = async () => {
            await g.press("demo-display");
            await g.press("demo-skin-picker");
            await g.press("demo-skin-picker/options/cybertron");
          };
          if (values.cdp) {
            const sampled = await optionalWorkerCpu(
              scenario.page.context().browser()!,
              scenario.page.workers().at(-1)!.url(),
              change,
            );
            cdpTrack = sampled.track;
            await writeFile(
              join(output, "worker-cpu-profile.json"),
              JSON.stringify(sampled.track),
            );
          } else await change();
          assert.equal((await g.state()).options.skin, "cybertron");
          const capture = await g.call<ProfileCapture>("stopTrace");
          await g.capture("trace-after");
          assert.ok(
            (await g.difference("trace-before", "trace-after")).changedPixels >
              100,
          );
          await save(
            capture,
            {
              directory: scenario.evidence.directory,
              browserVersion: scenario.page.context().browser()?.version(),
              action: "physical shared application skin selection",
            },
            {
              context: `application-page:${scenario.page.url()}`,
              timeOrigin: await scenario.page.evaluate(
                () => performance.timeOrigin,
              ),
            },
          );
        } finally {
          if (captureId) await g.call("cancelTrace", captureId);
          await scenario.page.evaluate(async () => {
            await window.ippDemoApplication?.dispose();
          });
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
            "shapes",
            ["--width", "960", "--height", "640"],
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
          await driver.call(
            "action",
            ["skin", '"cybertron"'],
            environment.signal,
          );
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
              nativeAction: "public application skin action",
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
