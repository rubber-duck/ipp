import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { arch, cpus, hostname, platform, release, totalmem } from "node:os";
import { resolve } from "node:path";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
  type BrowserEnvironmentContext,
} from "../browser/environment.js";
import { runNativeEnvironment } from "../integration/environment.js";
import {
  assertSeparators,
  measureSeparators,
} from "./surface-separators-scenario.js";
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
  type RetainedGuiDriver,
  TIMING_DEFINITIONS,
  type WorkloadFrame,
} from "./retained-gui-scenario.js";

/**
 * Where the runtime renders: a browser worker on WebGL, or the `gles_host`
 * testing example of `ipp-server` on native GLES, whose unchanged surface
 * fixture runs in a page as the generated client's JavaScript runtime. Only
 * this environment knows the arrangement; the scenario assertions do not.
 */
export type RetainedGuiArrangement =
  | { readonly kind: "worker" }
  | { readonly kind: "native-gles"; readonly eglDirectory: string };

/**
 * Both variants of an arrangement render through the same instrumentation
 * build: each simulates context loss, and the retained variant also bounds the
 * glyph atlas. Only the retained variant's fixture presents text through
 * retained GUI batches.
 */
const BUILDS = {
  worker: [
    { name: "render-instrumentation", variant: "analytic", retained: false },
    { name: "render-instrumentation", variant: "retained", retained: true },
  ],
  "native-gles": [
    { name: "gles", variant: "analytic", retained: false },
    { name: "gles", variant: "retained", retained: true },
  ],
} as const;

/**
 * Renderers that rasterize on the CPU. Their timings establish correctness in
 * that environment only and are never frame-rate evidence.
 */
const SOFTWARE_RENDERERS = /swiftshader|llvmpipe|softpipe|lavapipe/i;

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
  arrangement: RetainedGuiArrangement = { kind: "worker" },
) {
  const workspace = process.cwd();
  const identity = runIdentity(workspace, iterations);
  const builds = BUILDS[arrangement.kind];
  const reports: Array<
    {
      build: string;
      variant: string;
      /** Whether the device identity names a CPU rasterizer. */
      softwareRenderer: boolean;
      evidence: string;
      hostEvidence: string | null;
      browser: string | null;
      device: Record<string, unknown>;
    } & RetainedGuiReport
  > = [];
  const frames = new Map<string, Map<string, RgbaFrame>>();
  for (const { name, variant, retained } of builds) {
    const captured = new Map<string, RgbaFrame>();
    frames.set(variant, captured);
    const exercise = async (
      env: BrowserEnvironmentContext,
      connection: Record<string, unknown>,
    ) => {
      const module = `${env.urls.origin}/target/${retained ? "surface-gui-build" : "surface-build"}/fixture.js`;
      const call = <T>(name: string, args: readonly unknown[] = []) =>
        env.execute(name, args, () => invoke<T>(env.page, module, name, args));
      try {
        await call("initialize", [
          { generatedModuleUrl: env.urls.generated, ...connection },
        ]);
        const driver: RetainedGuiDriver = {
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
        };
        const report = await exerciseRetainedGui(
          driver,
          retained,
          iterations,
          options,
        );
        const separators = await measureSeparators(driver);
        await writeFile(
          resolve(env.evidence.directory, "separators.json"),
          JSON.stringify(separators, null, 2),
        );
        assertSeparators(separators);
        const device = Object.fromEntries(
          Object.entries(report.warm.statistics?.device ?? {}).filter(
            ([, value]) => typeof value === "string",
          ),
        );
        reports.push({
          build: name,
          variant,
          softwareRenderer: Object.values(device).some((value) =>
            SOFTWARE_RENDERERS.test(String(value)),
          ),
          evidence: env.evidence.directory,
          hostEvidence: null,
          browser: env.page.context().browser()?.version() ?? null,
          device,
          ...report,
        });
        await writeFile(
          resolve(env.evidence.directory, "workload.json"),
          JSON.stringify(report, null, 2),
        );
      } finally {
        await call("close");
      }
    };
    const browser = (build: BrowserBuildConfiguration, rendering: boolean) => ({
      workspace,
      build,
      rendering,
      deviceScaleFactor: 1,
      operationTimeoutMs: 30000,
      evidenceParent: output,
    });
    if (arrangement.kind === "worker") {
      const directory = resolve("target/browser-build", name);
      const build = {
        name,
        generatedModule: resolve(directory, "generated.js"),
        runtimeWasm: resolve(directory, "runtime.wasm"),
        contractArtifact: resolve(directory, "contract.bin"),
      };
      await runBrowserEnvironment(
        `retained-gui-${variant}`,
        browser(build, true),
        signal,
        (env) =>
          exercise(env, {
            workerScriptUrl: env.urls.workerScript,
            wasmUrl: env.urls.wasm,
          }),
      );
      continue;
    }

    // The page only runs the fixture; the native host renders and captures.
    const directory = resolve("target/gles-host-instrumentation");
    const executable = resolve(directory, "gles_host");
    const build = {
      name,
      generatedModule: resolve(directory, "generated.js"),
      runtimeWasm: executable,
      contractArtifact: resolve(directory, "contract.bin"),
    };
    const result = await runNativeEnvironment(
      `retained-gui-${variant}`,
      {
        executable,
        schemaArtifact: build.contractArtifact,
        workingDirectory: workspace,
        extraArguments: ["--egl-dir", arrangement.eglDirectory],
        readinessTimeoutMs: 30000,
        operationTimeoutMs: 30000,
        evidenceParent: output,
      },
      signal,
      async (native) => {
        const presentationUrl = native.presentationUrl;
        if (!presentationUrl)
          throw new Error("The GLES host named no presentation channel");
        await runBrowserEnvironment(
          `retained-gui-${variant}-client`,
          browser(build, false),
          native.signal,
          (env) =>
            exercise(env, { nativeHost: { url: native.url, presentationUrl } }),
        );
      },
    );
    reports.at(-1)!.hostEvidence = result.evidenceDirectory;
  }

  const [analyticBuild, retainedBuild] = builds.map(({ variant }) => variant);
  // Every label both builds captured is compared, with review artifacts kept
  // for passing and failing labels alike.
  const comparisons = compareBuildFrames(
    frames.get(analyticBuild!)!,
    frames.get(retainedBuild!)!,
  );
  const directory = resolve(output, "comparisons");
  await mkdir(directory, { recursive: true });
  for (const { label } of comparisons) {
    const expected = frames.get(analyticBuild!)!.get(label)!;
    const actual = frames.get(retainedBuild!)!.get(label)!;
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
        identity: { ...identity, arrangement: arrangement.kind },
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
        // Streamed-update timings per build; definitions beside them.
        timings: reports.map(({ build, variant, timings }) => ({
          build,
          variant,
          ...timings,
        })),
        timingDefinitions: TIMING_DEFINITIONS,
        timingScope:
          "Latencies through client, transport, scheduling and readback, never frame rates; builds whose softwareRenderer is true establish correctness only.",
        // Seven per-frame cache counters of each build's warm frame, running
        // totals after the last sample, and the warm frame's cache records.
        surfaceCache: options.surfaceCache
          ? reports.map(({ build, variant, surfaceCache }) => ({
              build,
              variant,
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
