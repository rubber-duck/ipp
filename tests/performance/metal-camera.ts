import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { cpus, loadavg, release } from "node:os";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { setTimeout } from "node:timers/promises";
import { renderDiagnostics } from "../../packages/ipp-client/src/diagnostics.js";
import { nativePresentationTransport } from "../../packages/ipp-client/src/native-presentation.js";
import { hostProfiling } from "../../packages/ipp-client/src/profiling.js";
import {
  encodePng,
  pixelDifference,
  type RgbaFrame,
} from "../harness/images.js";
import { runNativeEnvironment } from "../harness/native.js";
import { withMetalCamera } from "../rendering/scenarios/metal-camera.js";

const config: {
  source: { revision: string; sourceSha256: string };
  metal: string;
  gles: string;
  eglDirectory: string;
  repetitions: number;
  frames: number;
  instrumented: boolean;
  output: string;
} = JSON.parse(process.argv[2]!);
const out = resolve(config.output);
const viewport = { width: 1280, height: 720, devicePixelRatio: 1 };
await mkdir(out, { recursive: true });

const controller = new AbortController();
const interrupt = () => controller.abort();
process.once("SIGTERM", interrupt);
process.once("SIGINT", interrupt);

const pmset = (key: string) =>
  execFileSync("/usr/bin/pmset", ["-g", key], {
    encoding: "utf8",
  }).trim();
const machine = () => ({
  load: loadavg(),
  power: pmset("batt"),
  thermal: pmset("therm"),
});
const hash = async (file: string) =>
  createHash("sha256")
    .update(await readFile(file))
    .digest("hex");
const images: Partial<Record<"metal" | "gles", RgbaFrame>> = {};
const runs: unknown[] = [];
const failures: string[] = [];
const report = {
  config,
  viewport,
  cpu: cpus()[0]?.model,
  os: release(),
  runs,
  failures,
  pipelineRun: process.env.IPP_PIPELINE_RUN,
};
const save = () =>
  writeFile(join(out, "report.json"), JSON.stringify(report, null, 2));

function percentiles(data: readonly number[]) {
  const sorted = [...data].sort((a, b) => a - b);
  return {
    p50: sorted[Math.ceil(sorted.length * 0.5) - 1],
    p95: sorted[Math.ceil(sorted.length * 0.95) - 1],
  };
}

try {
  for (let repeat = 0; repeat < config.repetitions; repeat++) {
    const order =
      repeat % 2 === 0
        ? (["metal", "gles"] as const)
        : (["gles", "metal"] as const);
    for (const backend of order) {
      const product = resolve(config[backend]);
      const metal = backend === "metal";
      const executable = join(product, metal ? "ipp-metal-host" : "gles_host");
      const schemaArtifact = join(product, "contract.bin");
      const clientModule = join(product, "generated.js");
      const identity = {
        executable: await hash(executable),
        contract: await hash(schemaArtifact),
        client: await hash(clientModule),
      };
      const contract = (await import(
        pathToFileURL(clientModule).href
      )) as typeof import("@ipp/host-contract");
      const label = `${backend}-${repeat}`;
      try {
        const run = await runNativeEnvironment(
          label,
          {
            executable,
            schemaArtifact,
            workingDirectory: process.cwd(),
            evidenceParent: out,
            readinessTimeoutMs: 30_000,
            operationTimeoutMs: 35_000,
            environment: { IPP_LOG: "info" },
            extraArguments: [
              "--uncapped",
              ...(metal ? [] : ["--egl-dir", config.eglDirectory]),
            ],
          },
          controller.signal,
          async (environment) => {
            const host = metal
              ? await contract.IppHostClient.connectWebSocket(environment.url)
              : await contract.IppHostClient.connectTransport(
                  nativePresentationTransport(
                    environment.url,
                    environment.presentationUrl!,
                  ),
                );
            try {
              return await withMetalCamera(
                host as unknown as Parameters<typeof withMetalCamera>[0],
                async (output) => {
                  const binding = await host.setRootOutput(output, viewport);
                  const view = await host.presentation.select(
                    await host.presentation.surface(),
                    binding,
                  );
                  let sequence = 0n;
                  const frame = async () => {
                    const completed = await host.presentation.frame(view, {
                      afterOutputs: [output],
                      afterSequence: sequence,
                    });
                    assert.ok(completed.sequence > sequence);
                    assert.equal(completed.failedDrawCalls, 0);
                    assert.equal(completed.triangles, 112);
                    assert.equal(completed.drawCalls, metal ? 5 : 7);
                    sequence = completed.sequence;
                    return completed;
                  };
                  await frame();
                  const device = metal
                    ? (
                        await readFile(
                          join(
                            environment.evidence.directory,
                            "server-stderr.log",
                          ),
                          "utf8",
                        )
                      ).match(/\[IPP Metal\] device=(.+)/)?.[1]
                    : (await renderDiagnostics(host)?.statistics())?.device
                        .renderer;
                  assert.match(String(device), /Apple|Metal/i);
                  assert.doesNotMatch(String(device), /swiftshader|llvmpipe/i);
                  const before = machine();
                  const warm = performance.now();
                  do {
                    await frame();
                  } while (performance.now() - warm < 1000);
                  const intervals: number[] = [];
                  const sequences: string[] = [];
                  let timing: unknown = null;
                  if (config.instrumented) {
                    const profiler = hostProfiling(host);
                    const id = await profiler.start({
                      counters: false,
                      gpu: "frame",
                    });
                    try {
                      for (let i = 0; i < Math.min(config.frames, 32); i++)
                        await frame();
                      const gpu = await profiler.stop();
                      await writeFile(
                        join(out, `${label}-profile.json`),
                        JSON.stringify(gpu, null, 2),
                      );
                    } finally {
                      await profiler.release(id);
                    }
                  } else {
                    const first = sequence;
                    const start = performance.now();
                    let previous = start;
                    do {
                      await frame();
                      const current = performance.now();
                      intervals.push(current - previous);
                      sequences.push(String(sequence));
                      previous = current;
                    } while (
                      intervals.length < config.frames ||
                      previous - start < 2500
                    );
                    const elapsedMs = previous - start;
                    timing = {
                      elapsedMs,
                      completedDraws: Number(sequence - first),
                      drawsPerSecond:
                        (Number(sequence - first) * 1000) / elapsedMs,
                      sampledResponseIntervalMs: percentiles(intervals),
                    };
                  }
                  const capture = await host.presentation.capture(view, {
                    afterOutputs: [output],
                    afterSequence: sequence,
                  });
                  assert.equal(capture.triangles, 112);
                  assert.equal(capture.failedDrawCalls, 0);
                  const image = {
                    width: viewport.width,
                    height: viewport.height,
                    pixels: new Uint8Array(capture.pixels),
                  };
                  const png = `${label}.png`;
                  await writeFile(join(out, png), encodePng(image));
                  images[backend] ??= image;
                  const reference = images[backend];
                  assert.deepEqual(
                    image.pixels,
                    reference.pixels,
                    `Stable ${backend} image`,
                  );
                  const other = images[metal ? "gles" : "metal"];
                  let pixelsOverTwo = 0;
                  if (other) {
                    for (
                      let pixel = 0;
                      pixel < viewport.width * viewport.height;
                      pixel++
                    )
                      pixelsOverTwo += Number(
                        pixelDifference(image, other, pixel) > 2,
                      );
                    assert.ok(
                      pixelsOverTwo <= 100,
                      `Reference image differs at ${pixelsOverTwo} pixels`,
                    );
                  }
                  await host.presentation.clear(view);
                  return {
                    label,
                    backend,
                    repeat,
                    device,
                    identity,
                    before,
                    after: machine(),
                    timing,
                    intervals,
                    sequences,
                    png,
                    pixelsOverTwo,
                    drawCalls: capture.drawCalls,
                    triangles: capture.triangles,
                  };
                },
              );
            } finally {
              await host.close();
            }
          },
        );
        runs.push({ ...run.value, evidence: run.evidenceDirectory });
      } catch (error) {
        failures.push(error instanceof Error ? error.message : String(error));
      }
      await save();
      await setTimeout(200, undefined, { signal: controller.signal });
    }
  }
  assert.deepEqual(failures, []);
} finally {
  process.removeListener("SIGTERM", interrupt);
  process.removeListener("SIGINT", interrupt);
}
