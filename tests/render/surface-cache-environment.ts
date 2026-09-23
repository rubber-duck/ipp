import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { runBrowserEnvironment } from "../browser/environment.js";
import { invoke } from "./evidence.js";
import { encodePng, type RgbaFrame } from "./retained-gui-images.js";
import {
  exerciseSurfaceCache,
  type CacheFrame,
  type SurfaceCacheReport,
} from "./surface-cache-scenario.js";

/** Raw Surfaces alone prove lean gating; the GUI build adds interaction priority. */
export const SURFACE_CACHE_BUILDS = [
  { name: "render-surfaces", gui: false },
  { name: "headless-gui", gui: true },
] as const;

export interface SurfaceCacheBuildReport {
  build: string;
  evidence: string;
  browser: string | null;
  report: SurfaceCacheReport;
}

/** Launch each build's worker/WASM/WebGL runtime and drive the shared scenario. */
export async function runSurfaceCache(
  signal: AbortSignal,
  output: string,
): Promise<SurfaceCacheBuildReport[]> {
  const workspace = process.cwd();
  const reports: SurfaceCacheBuildReport[] = [];
  for (const { name, gui } of SURFACE_CACHE_BUILDS) {
    const directory = resolve("target/browser-build", name);
    const build = {
      name,
      generatedModule: resolve(directory, "generated.js"),
      runtimeWasm: resolve(directory, "runtime.wasm"),
      exportWasm: resolve(directory, "export.wasm"),
      contractArtifact: resolve(directory, "contract.bin"),
    };
    const captured = new Map<string, RgbaFrame>();
    await runBrowserEnvironment(
      `surface-cache-${name}`,
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
        const call = <T>(operation: string, args: readonly unknown[] = []) =>
          env.execute(operation, args, () =>
            invoke<T>(env.page, module, operation, args),
          );
        try {
          await call("initialize", [
            {
              generatedModuleUrl: env.urls.generated,
              workerScriptUrl: env.urls.workerScript,
              wasmUrl: env.urls.wasm,
            },
          ]);
          const report = await exerciseSurfaceCache({
            build: name,
            gui,
            call,
            capture: async (label, next = false) => {
              const frame = await call<CacheFrame>("capture", [
                label,
                { next },
              ]);
              // Pixels stay in Node for comparisons; the event log keeps the path.
              await env.execute(`write ${label}.png`, [label], async () => {
                const { width, height, pixels } = await invoke<{
                  width: number;
                  height: number;
                  pixels: string;
                }>(env.page, module, "capturePixels", [label]);
                const image = {
                  width,
                  height,
                  pixels: new Uint8Array(Buffer.from(pixels, "base64")),
                };
                captured.set(label, image);
                const path = resolve(env.evidence.directory, `${label}.png`);
                await writeFile(path, encodePng(image));
                return path;
              });
              return frame;
            },
            pixels: (label) => {
              const frame = captured.get(label);
              if (!frame) throw new Error(`No capture labelled ${label}`);
              return frame;
            },
          });
          reports.push({
            build: name,
            evidence: env.evidence.directory,
            browser: env.page.context().browser()?.version() ?? null,
            report,
          });
          await writeFile(
            resolve(env.evidence.directory, "surface-cache.json"),
            JSON.stringify(report, null, 2),
          );
        } finally {
          await call("close");
        }
      },
    );
  }
  await mkdir(output, { recursive: true });
  await writeFile(
    resolve(output, "summary.json"),
    JSON.stringify(
      reports.map(({ build, evidence, browser, report }) => ({
        build,
        evidence,
        browser,
        status: report.status,
      })),
      null,
      2,
    ),
  );
  return reports;
}
