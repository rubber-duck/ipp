import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { runBrowserEnvironment } from "../browser/environment.js";
import { invoke } from "./evidence.js";
import { encodePng, type RgbaFrame } from "./retained-gui-images.js";
import {
  exerciseSurfaceCache,
  measureCacheDensity,
  type CacheDensityReport,
  type CacheFrame,
  type SurfaceCacheDriver,
  type SurfaceCacheReport,
} from "./surface-cache-scenario.js";

/** Raw Surfaces alone, then the GUI fixture's interaction priority, on one build. */
export const SURFACE_CACHE_BUILDS = [
  { name: "render-instrumentation", label: "surfaces", gui: false },
  { name: "render-instrumentation", label: "gui", gui: true },
] as const;

/** Playwright device scale factors of the cached versus direct density measurement. */
export const DENSITY_DEVICE_PIXEL_RATIOS = [1, 2] as const;

export interface SurfaceCacheBuildReport {
  build: string;
  evidence: string;
  browser: string | null;
  report: SurfaceCacheReport;
  /** Cached versus direct text per device-pixel ratio, each in its own page. */
  density: (CacheDensityReport & { evidence: string })[];
}

/** Launch each build's worker/WASM/WebGL runtime and drive the shared scenario. */
export async function runSurfaceCache(
  signal: AbortSignal,
  output: string,
  builds: readonly (typeof SURFACE_CACHE_BUILDS)[number][] = SURFACE_CACHE_BUILDS,
): Promise<SurfaceCacheBuildReport[]> {
  const workspace = process.cwd();
  const reports: SurfaceCacheBuildReport[] = [];
  for (const { name, label: variant, gui } of builds) {
    const directory = resolve("target/browser-build", name);
    const build = {
      name,
      generatedModule: resolve(directory, "generated.js"),
      runtimeWasm: resolve(directory, "runtime.wasm"),
      contractArtifact: resolve(directory, "contract.bin"),
    };
    /** Run `scenario` in a fresh page at `deviceScaleFactor` with the shared driver. */
    const session = <T>(
      label: string,
      deviceScaleFactor: number,
      scenario: (
        driver: SurfaceCacheDriver,
        page: { evidence: string; browser: string | null },
      ) => Promise<T>,
    ) => {
      const captured = new Map<string, RgbaFrame>();
      return runBrowserEnvironment(
        label,
        {
          workspace,
          build,
          rendering: true,
          deviceScaleFactor,
          operationTimeoutMs: 30000,
          evidenceParent: output,
        },
        signal,
        async (env) => {
          const module = `${env.urls.origin}/target/${gui ? "surface-gui-build" : "surface-build"}/fixture.js`;
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
            return await scenario(
              {
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
                    const path = resolve(
                      env.evidence.directory,
                      `${label}.png`,
                    );
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
              },
              {
                evidence: env.evidence.directory,
                browser: env.page.context().browser()?.version() ?? null,
              },
            );
          } finally {
            await call("close");
          }
        },
      ).then(({ value }) => value);
    };
    const { report, evidence, browser } = await session(
      `surface-cache-${variant}`,
      1,
      async (driver, page) => {
        const report = await exerciseSurfaceCache(driver);
        await writeFile(
          resolve(page.evidence, "surface-cache.json"),
          JSON.stringify(report, null, 2),
        );
        return { report, ...page };
      },
    );
    const density = [];
    for (const ratio of DENSITY_DEVICE_PIXEL_RATIOS)
      density.push(
        await session(
          `surface-cache-${variant}-dpr${ratio}`,
          ratio,
          async (driver, { evidence }) => ({
            ...(await measureCacheDensity(driver)),
            evidence,
          }),
        ),
      );
    reports.push({ build: variant, evidence, browser, report, density });
  }
  await mkdir(output, { recursive: true });
  await writeFile(
    resolve(output, "summary.json"),
    JSON.stringify(
      reports.map(({ build, evidence, browser, report, density }) => ({
        build,
        evidence,
        browser,
        status: report.status,
        density,
      })),
      null,
      2,
    ),
  );
  return reports;
}
