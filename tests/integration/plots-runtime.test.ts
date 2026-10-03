/** Real native GLES and worker/WebGL drivers share Plot fixtures and assertions. */
import test from "node:test";
import { readFile, writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import { pathToFileURL } from "node:url";
import type { Client, HostClientBase, RootBinding } from "@ipp/client";
import { image, settled } from "../../tools/shared-host/presentation.js";
import { encodePng } from "../../tools/shared-host/png.js";
import { runNativeEnvironment } from "./environment.js";
import { runBrowserEnvironment } from "../browser/environment.js";
import { exercisePlot2d } from "./scenarios/plots-2d.js";
import { exercisePlot3d } from "./scenarios/plots-3d.js";
import { reactPlots } from "./scenarios/react-plots.js";
import { exercisePlotViewPlacement } from "./scenarios/plot-view-placement.js";
import type { Plot2dContract } from "./plot-2d-scene.js";
import type { Plot3dContract } from "./plot-3d-scene.js";

for (const family of ["2d", "3d", "react", "view"] as const) {
  test(`${family} Plot through native WebSocket/GLES`, {
    timeout: 180_000,
  }, async (context) => {
    const workspace = process.cwd();
    const profile = resolve(workspace, "target/gles-host");
    const contract = (await import(
      pathToFileURL(join(profile, "generated.js")).href
    )) as Plot2dContract &
      Plot3dContract & {
        IppHostClient: {
          connectWebSocket(
            url: string,
            options: { signal: AbortSignal },
          ): Promise<HostClientBase<Client>>;
        };
      };
    const font = new Uint8Array(
      await readFile(
        resolve(workspace, "target/font-assets/shure-tech-mono.ippf"),
      ),
    );
    await runNativeEnvironment(
      `plots-${family}`,
      {
        executable: join(
          profile,
          process.platform === "win32" ? "gles_host.exe" : "gles_host",
        ),
        schemaArtifact: join(profile, "contract.bin"),
        workingDirectory: workspace,
        extraArguments: [
          "--egl-dir",
          process.env.IPP_EGL_LIBRARY_DIR ?? "/lib64",
        ],
        readinessTimeoutMs: 30_000,
        operationTimeoutMs: 150_000,
        evidenceParent: resolve(
          workspace,
          "target/integration-artifacts/plots",
        ),
      },
      context.signal,
      async (environment) => {
        const host = await environment.track(
          contract.IppHostClient.connectWebSocket(environment.url, {
            signal: environment.signal,
          }),
        );
        const surface = await host.presentation.surface();
        const capture = async (label: string, binding: RootBinding) => {
          const view = await host.presentation.select(surface, binding);
          try {
            const frame = image(await settled(host, view, [binding.output]));
            await writeFile(
              join(environment.evidence.directory, `${label}.png`),
              encodePng(frame),
            );
            return frame;
          } finally {
            await host.presentation.clear(view);
          }
        };
        const record = (label: string, value: unknown) =>
          environment.evidence.record(label, value);
        await environment.execute(`plots-${family}`, {}, async () =>
          family === "2d"
            ? exercisePlot2d(host, contract, font, capture, record)
            : family === "3d"
              ? exercisePlot3d(host, contract, font, capture)
              : family === "view"
                ? exercisePlotViewPlacement(
                    host,
                    contract,
                    font,
                    capture,
                    record,
                  )
                : reactPlots(host, contract, font, capture, record),
        );
      },
    );
  });

  test(`${family} Plot through worker WASM/WebGL`, {
    timeout: 180_000,
  }, async (context) => {
    const workspace = process.cwd();
    const profile = resolve(workspace, "target/browser-build/render");
    await runBrowserEnvironment(
      `plots-${family}-webgl`,
      {
        workspace,
        build: {
          name: "render",
          generatedModule: join(profile, "generated.js"),
          runtimeWasm: join(profile, "runtime.wasm"),
          contractArtifact: join(profile, "contract.bin"),
        },
        operationTimeoutMs: 150_000,
        evidenceParent: resolve(
          workspace,
          "target/integration-artifacts/plots",
        ),
      },
      context.signal,
      async (environment) => {
        await environment.page.exposeFunction(
          "recordPlot",
          async (label: string, value: unknown) => {
            if (label === "capture") {
              const frame = value as {
                label: string;
                width: number;
                height: number;
                pixels: number[];
              };
              await writeFile(
                join(environment.evidence.directory, `${frame.label}.png`),
                encodePng({ ...frame, pixels: Uint8Array.from(frame.pixels) }),
              );
              await environment.evidence.record(label, {
                label: frame.label,
                width: frame.width,
                height: frame.height,
              });
            } else await environment.evidence.record(label, value);
          },
        );
        await environment.execute(`plots-${family}-webgl`, {}, () =>
          environment.page.evaluate(
            async ({ urls, family }) => {
              const driver = await import(
                `${urls.origin}/target/plots/plots-driver.js`
              );
              return driver.workerPlots(urls, family);
            },
            { urls: environment.urls, family },
          ),
        );
      },
    );
  });
}
