/** Release native driver. Host clocks run normally; measurements never advance time. */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { createWriteStream } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { cpus, platform, release } from "node:os";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import type { Client, HostClientBase } from "@ipp/client";
import { nativePresentationTransport } from "@ipp/client/testing";
import { renderDiagnostics } from "@ipp/client/diagnostics";
import { encodePng } from "../harness/images.js";
import { image } from "../../tools/shared-host/presentation.js";
import {
  openWorkload,
  workloads,
  type ChartContract,
} from "../data/scenarios/chart-data-workload.js";

interface Configuration {
  preset: "smoke" | "full";
  samples: number;
  repetitions: number;
  warmup: number;
  cases: string | null;
  modes: string | null;
  eglDir: string;
  allowSoftware: boolean;
  output: string;
}
const config = JSON.parse(process.argv[2]!) as Configuration;
assert.ok(config.samples >= 1 && config.samples <= 64);
assert.ok(config.repetitions >= 1 && config.repetitions <= 8);
assert.ok(config.warmup >= 0 && config.warmup <= 16);
const directory = resolve(config.output);
await mkdir(directory, { recursive: true });
const product = resolve("target/performance-build/chart-data");
const build = JSON.parse(
  await readFile(resolve(product, "build-identity.json"), "utf8"),
);
assert.equal(build.profile, "release");
assert.equal(build.instrumented, false);
assert.equal(
  createHash("sha256")
    .update(await readFile(resolve(product, "contract.bin")))
    .digest("hex"),
  build.contractSha256,
);
assert.equal(
  createHash("sha256")
    .update(await readFile(resolve(product, "chart-data.js")))
    .digest("hex"),
  build.fixtureSha256,
);
const executable = resolve(
  product,
  process.platform === "win32" ? "gles_host.exe" : "gles_host",
);
assert.equal(
  createHash("sha256")
    .update(await readFile(executable))
    .digest("hex"),
  build.executableSha256,
);
const stdout = createWriteStream(resolve(directory, "host-stdout.log"));
const stderr = createWriteStream(resolve(directory, "host-stderr.log"));
const child = spawn(
  executable,
  ["--bind", "127.0.0.1:0", "--egl-dir", config.eglDir],
  { stdio: ["ignore", "pipe", "pipe"] },
);
child.stdout.pipe(stdout);
child.stderr.pipe(stderr);
let host: HostClientBase<Client> | undefined;
const results: unknown[] = [];
let failure: unknown;
const abort = new AbortController();
const deadline = setTimeout(
  () => abort.abort(new Error("Bounded chart/data run exceeded 12 minutes")),
  720000,
);
const interruption = () => abort.abort(new Error("Benchmark interrupted"));
process.once("SIGINT", interruption);
process.once("SIGTERM", interruption);
const stop = () => child.kill("SIGTERM");
abort.signal.addEventListener("abort", stop, { once: true });

function json(value: unknown) {
  return JSON.stringify(
    value,
    (_, item) => (typeof item === "bigint" ? item.toString() : item),
    2,
  );
}

/** Quantiles describe measured operation latency, including Host scheduling/transport. */
function quantiles(values: readonly number[]) {
  const sorted = [...values].sort((a, b) => a - b);
  const at = (fraction: number) =>
    sorted[Math.max(0, Math.ceil(sorted.length * fraction) - 1)]!;
  return {
    count: values.length,
    p50Ms: at(0.5),
    p95Ms: at(0.95),
    minMs: sorted[0],
    maxMs: sorted.at(-1),
  };
}

async function rssBytes() {
  if (process.platform !== "linux") return null;
  const status = await readFile(`/proc/${child.pid}/status`, "utf8");
  const match = /^VmRSS:\s+(\d+)\s+kB$/m.exec(status);
  return match ? Number(match[1]) * 1024 : null;
}

try {
  const readiness = await new Promise<{ url: string; presentation: string }>(
    (accept, reject) => {
      let buffer = "";
      const timer = setTimeout(
        () => reject(new Error("Native readiness timeout")),
        30000,
      );
      const fail = (error: Error) => {
        clearTimeout(timer);
        reject(error);
      };
      child.once("error", fail);
      child.once("exit", (code) =>
        fail(new Error(`Host exited before readiness: ${code}`)),
      );
      child.stdout.on("data", (chunk: Buffer) => {
        buffer += chunk.toString();
        for (;;) {
          const end = buffer.indexOf("\n");
          if (end < 0) break;
          const line = buffer.slice(0, end);
          buffer = buffer.slice(end + 1);
          try {
            const value = JSON.parse(line);
            if (value.event === "ready") {
              clearTimeout(timer);
              accept(value);
            }
          } catch {
            /* Ordinary diagnostic lines are retained in Host logs. */
          }
        }
      });
    },
  );
  const generated = (await import(
    pathToFileURL(resolve(product, "generated.js")).href
  )) as ChartContract & {
    IppHostClient: {
      connectTransport: (
        transport: ReturnType<typeof nativePresentationTransport>,
        options: { signal: AbortSignal; timeoutMs: number },
      ) => Promise<HostClientBase<Client>>;
    };
  };
  host = await generated.IppHostClient.connectTransport(
    nativePresentationTransport(readiness.url, readiness.presentation),
    { signal: abort.signal, timeoutMs: 30000 },
  );
  const font = new Uint8Array(
    await readFile(resolve("target/font-assets/shure-tech-mono.ippf")),
  );
  const selected = workloads(config.preset);
  const requested = config.cases?.split(",");
  if (requested)
    for (const name of requested)
      assert.ok(
        selected.some((workload) => workload.name === name),
        `Unknown case ${name}`,
      );
  const surface = await host.presentation.surface();
  for (let repeat = 0; repeat < config.repetitions; repeat++) {
    for (const workload of selected.filter(
      (value) => !requested || requested.includes(value.name),
    )) {
      abort.signal.throwIfAborted();
      const setupStart = performance.now();
      const fixture = await openWorkload(
        host,
        generated,
        font,
        workload,
        `${process.pid}-${repeat}`,
      );
      const view = await host.presentation.select(surface, fixture.binding);
      try {
        const initial = await host.presentation.capture(view, {
          afterOutputs: [fixture.binding.output],
        });
        await writeFile(
          resolve(directory, `${workload.name}-${repeat}-initial.png`),
          encodePng(image(initial)),
        );
        assert.equal(initial.failedDrawCalls, 0);
        assert.ok(initial.drawCalls > 0 && initial.triangles > 0);
        const diagnostics = renderDiagnostics(host)!;
        const before = await diagnostics.statistics();
        const renderer = String(
          before.device.renderer ?? before.device.unmaskedRenderer ?? "",
        );
        if (
          /llvmpipe|softpipe|swiftshader|lavapipe/i.test(renderer) &&
          !config.allowSoftware
        )
          throw new Error(
            `Software GLES requires explicit --allow-software: ${renderer}`,
          );
        const setupMs = performance.now() - setupStart;
        const modes = [
          "idle",
          "edit",
          "parameter",
          "animation",
          ...(fixture.spatial ? ["camera"] : []),
        ].filter(
          (mode) => !config.modes || config.modes.split(",").includes(mode),
        );
        for (const mode of modes) {
          let sequence = initial.sequence;
          for (let cycle = 0; cycle < config.warmup; cycle++) {
            await fixture.action(mode, cycle);
            const frame = await host.presentation.frame(view, {
              afterOutputs: [fixture.binding.output],
              afterSequence: sequence,
            });
            sequence = frame.sequence;
          }
          const memoryBefore = await fixture.verify();
          const rssBefore = await rssBytes();
          const statisticsBefore = await diagnostics.statistics();
          const samples = [];
          for (let cycle = 0; cycle < config.samples; cycle++) {
            abort.signal.throwIfAborted();
            const start = performance.now();
            await fixture.action(mode, cycle + config.warmup);
            const acknowledged = performance.now();
            const frame = await host.presentation.frame(view, {
              afterOutputs: [fixture.binding.output],
              afterSequence: sequence,
            });
            const completed = performance.now();
            assert.ok(frame.sequence > sequence);
            assert.equal(frame.failedDrawCalls, 0);
            assert.ok(
              frame.drawCalls > 0 && frame.triangles > 0,
              `${workload.name}/${mode} completed an empty frame`,
            );
            sequence = frame.sequence;
            samples.push({
              acknowledgedMs: acknowledged - start,
              completedMs: completed - start,
              sequence: frame.sequence.toString(),
              tick: frame.sources.map((source) => source.tick.toString()),
              drawCalls: frame.drawCalls,
              triangles: frame.triangles,
            });
          }
          const statisticsAfter = await diagnostics.statistics();
          const rssAfter = await rssBytes();
          const memoryAfter = await fixture.verify();
          results.push({
            workload,
            repeat,
            mode,
            setupMs,
            timing: quantiles(samples.map((sample) => sample.completedMs)),
            acknowledgement: quantiles(
              samples.map((sample) => sample.acknowledgedMs),
            ),
            samples,
            memoryBefore,
            memoryAfter,
            rssBefore,
            rssAfter,
            statisticsBefore,
            statisticsAfter,
          });
          await writeFile(
            resolve(directory, "results.json"),
            json({ config, build, results }),
          );
        }
        const capture = await host.presentation.capture(view, {
          afterOutputs: [fixture.binding.output],
        });
        const frame = image(capture);
        const colors = new Set<number>();
        let cyan = 0;
        for (let at = 0; at < frame.pixels.length; at += 4) {
          const r = frame.pixels[at]!,
            g = frame.pixels[at + 1]!,
            b = frame.pixels[at + 2]!;
          colors.add((r << 16) | (g << 8) | b);
          if (g > 90 && b > 120 && r < g / 2) cyan++;
        }
        await writeFile(
          resolve(directory, `${workload.name}-${repeat}.png`),
          encodePng(frame),
        );
        await writeFile(
          resolve(directory, `${workload.name}-${repeat}-frame.json`),
          json({
            sequence: capture.sequence,
            sources: capture.sources,
            drawCalls: capture.drawCalls,
            triangles: capture.triangles,
            colors: colors.size,
            cyanPixels: cyan,
          }),
        );
        assert.equal(capture.failedDrawCalls, 0);
        assert.ok(
          colors.size > 8 && cyan > 10,
          "Completed native frame contains no meaningful chart paint",
        );
      } finally {
        await host.presentation.clear(view).catch(() => {});
        await fixture
          .close()
          .catch((error) => console.error("Fixture cleanup failed", error));
      }
    }
  }
} catch (error) {
  failure = error;
} finally {
  clearTimeout(deadline);
  process.removeListener("SIGINT", interruption);
  process.removeListener("SIGTERM", interruption);
  await host?.close().catch(() => {});
  if (child.exitCode === null && child.signalCode === null) {
    const exited = new Promise<void>((accept) =>
      child.once("exit", () => accept()),
    );
    child.kill("SIGTERM");
    const kill = setTimeout(() => child.kill("SIGKILL"), 3000);
    await exited;
    clearTimeout(kill);
  }
  stdout.end();
  stderr.end();
  await writeFile(
    resolve(directory, "results.json"),
    json({
      config,
      build,
      machine: {
        cpu: cpus()[0]?.model,
        cores: cpus().length,
        platform: platform(),
        release: release(),
        node: process.version,
      },
      hostPid: child.pid,
      hostExit: { code: child.exitCode, signal: child.signalCode },
      termination: abort.signal.aborted ? String(abort.signal.reason) : null,
      metric:
        "native command acknowledgement and next completed frame; includes transport and Host scheduling, excludes pixel capture",
      results,
      failure: failure instanceof Error ? failure.stack : (failure ?? null),
    }),
  );
}
if (failure) throw failure;
