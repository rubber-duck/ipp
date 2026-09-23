import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { arch, cpus, hostname, platform, release, totalmem } from "node:os";
import { resolve } from "node:path";
import { runBrowserEnvironment } from "../browser/environment.js";
import { invoke } from "./evidence.js";
import {
  differenceImage,
  encodePng,
  type RgbaFrame,
} from "./retained-gui-images.js";
import {
  assertBuildComparisons,
  COMPARISON_TOLERANCE,
  compareBuildFrames,
  exerciseRetainedGui,
  type RetainedGuiOptions,
  type RetainedGuiReport,
  type WorkloadFrame,
} from "./retained-gui-scenario.js";

/** Both builds render; only the GUI build presents text through retained batches. */
const BUILDS = [
  { name: "render-surfaces", retained: false },
  { name: "headless-gui", retained: true },
] as const;

/** Source, machine and pipeline identity that make retained evidence comparable. */
function runIdentity(workspace: string, iterations: number) {
  const git = (...args: string[]) =>
    execFileSync("git", args, {
      cwd: workspace,
      maxBuffer: 1 << 28,
    });
  const status = git("status", "--porcelain=v1", "-z").toString();
  const changes = createHash("sha256")
    .update(git("diff", "HEAD", "--binary"))
    .update(status)
    .digest("hex");
  const processors = cpus();
  return {
    source: {
      revision: git("rev-parse", "HEAD").toString().trim(),
      uncommittedChanges: status.length > 0,
      // Tracked diff and untracked names; equal only for the same edits.
      changesSha256: status.length > 0 ? changes : null,
    },
    machine: {
      hostname: hostname(),
      platform: platform(),
      release: release(),
      arch: arch(),
      cpu: processors[0]?.model ?? null,
      logicalCpus: processors.length,
      memoryBytes: totalmem(),
      node: process.version,
    },
    pipelineRun: process.env.IPP_PIPELINE_RUN ?? null,
    startedAt: new Date().toISOString(),
    streamingUpdates: iterations,
  };
}

export async function runRetainedGui(
  signal: AbortSignal,
  iterations: number,
  output: string,
  options: RetainedGuiOptions = {},
) {
  const workspace = process.cwd();
  const identity = runIdentity(workspace, iterations);
  const reports: Array<
    {
      build: string;
      evidence: string;
      browser: string | null;
      backend: Record<string, unknown>;
    } & RetainedGuiReport
  > = [];
  const frames = new Map<string, Map<string, RgbaFrame>>();
  for (const { name, retained } of BUILDS) {
    const directory = resolve("target/browser-build", name);
    const build = {
      name,
      generatedModule: resolve(directory, "generated.js"),
      runtimeWasm: resolve(directory, "runtime.wasm"),
      exportWasm: resolve(directory, "export.wasm"),
      contractArtifact: resolve(directory, "contract.bin"),
    };
    const captured = new Map<string, RgbaFrame>();
    frames.set(name, captured);
    await runBrowserEnvironment(
      `retained-gui-${name}`,
      {
        workspace,
        build,
        mismatchBuild: build,
        rendering: true,
        deviceScaleFactor: 1,
        operationTimeoutMs: 30000,
        evidenceParent: output,
      },
      signal,
      async (env) => {
        const module = `${env.urls.origin}/target/surface-build/fixture.js`;
        const call = <T>(name: string, args: readonly unknown[] = []) =>
          env.execute(name, args, () =>
            invoke<T>(env.page, module, name, args),
          );
        try {
          await call("initialize", [
            {
              generatedModuleUrl: env.urls.generated,
              workerScriptUrl: env.urls.workerScript,
              wasmUrl: env.urls.wasm,
            },
          ]);
          const report = await exerciseRetainedGui(
            {
              call,
              capture: async (label, next = false) => {
                const result = await call<WorkloadFrame>("capture", [
                  label,
                  { next },
                ]);
                // Keep pixels for cross-build comparison and record the artifact path;
                // PNG data would exhaust the event log.
                await env.execute(`write ${label}.png`, [label], async () => {
                  const { width, height, pixels } = await invoke<{
                    width: number;
                    height: number;
                    pixels: string;
                  }>(env.page, module, "capturePixels", [label]);
                  const frame = {
                    width,
                    height,
                    pixels: new Uint8Array(Buffer.from(pixels, "base64")),
                  };
                  captured.set(label, frame);
                  const path = resolve(env.evidence.directory, `${label}.png`);
                  await writeFile(path, encodePng(frame));
                  return path;
                });
                return result;
              },
              pixels: (label) => {
                const frame = captured.get(label);
                if (!frame) throw new Error(`No capture labelled ${label}`);
                return frame;
              },
            },
            retained,
            iterations,
            options,
          );
          reports.push({
            build: name,
            evidence: env.evidence.directory,
            browser: env.page.context().browser()?.version() ?? null,
            backend: Object.fromEntries(
              Object.entries(report.warm.backend).filter(
                ([, value]) => typeof value === "string",
              ),
            ),
            ...report,
          });
          await writeFile(
            resolve(env.evidence.directory, "workload.json"),
            JSON.stringify(report, null, 2),
          );
        } finally {
          await call("close");
        }
      },
    );
  }

  // Every label both builds captured is compared, with review artifacts kept
  // for passing and failing labels alike.
  const comparisons = compareBuildFrames(
    frames.get("render-surfaces")!,
    frames.get("headless-gui")!,
  );
  const directory = resolve(output, "comparisons");
  await mkdir(directory, { recursive: true });
  for (const { label } of comparisons) {
    const expected = frames.get("render-surfaces")!.get(label)!;
    const actual = frames.get("headless-gui")!.get(label)!;
    await Promise.all([
      writeFile(
        resolve(directory, `${label}-expected.png`),
        encodePng(expected),
      ),
      writeFile(resolve(directory, `${label}-actual.png`), encodePng(actual)),
      writeFile(
        resolve(directory, `${label}-diff.png`),
        encodePng(
          differenceImage(
            expected,
            actual,
            COMPARISON_TOLERANCE.channelThreshold,
          ),
        ),
      ),
    ]);
  }
  const [analytic, retained] = reports;
  await writeFile(
    resolve(output, "comparison.json"),
    JSON.stringify(
      {
        identity,
        quality: {
          devicePixelRatio: retained!.devicePixelRatio,
          viewports: retained!.viewports,
          terminal: retained!.terminal,
          atlas: {
            rows: retained!.atlas.rows,
            columns: retained!.atlas.columns,
            pageBudget: retained!.atlas.pageBudget,
          },
        },
        tolerance: COMPARISON_TOLERANCE,
        // Seven per-frame cache counters of each build's warm frame, running
        // totals after the last sample, and the warm frame's cache records.
        surfaceCache: options.surfaceCache
          ? reports.map(({ build, surfaceCache }) => ({
              build,
              ...surfaceCache,
            }))
          : null,
        comparisons: comparisons.map((comparison) => ({
          ...comparison,
          artifacts: ["expected", "actual", "diff"].map(
            (kind) => `comparisons/${comparison.label}-${kind}.png`,
          ),
        })),
        builds: reports,
      },
      null,
      2,
    ),
  );
  assertBuildComparisons(comparisons);
  if (analytic!.devicePixelRatio !== retained!.devicePixelRatio)
    throw new Error("Analytic and retained builds used different DPR");
  return reports;
}
