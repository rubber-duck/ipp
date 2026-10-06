/** Real native GLES and worker/WebGL drivers share Plot fixtures and assertions. */
import test from "node:test";
import { readFile, writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import { pathToFileURL } from "node:url";
import type { Client, HostClientBase } from "@ipp/client";
import { createPlotCapture } from "./support/plot-capture.js";
import { encodePng } from "../harness/images.js";
import { runNativeEnvironment } from "../harness/native.js";
import { runBrowserEnvironment } from "../harness/browser.js";
import { exercisePlot2d } from "./scenarios/plots-2d.js";
import { exerciseDenseLines } from "./scenarios/dense-lines.js";
import { exercisePlot3d } from "./scenarios/plots-3d.js";
import { reactPlots } from "./scenarios/react-plots.js";
import { exercisePlotAxisSupport } from "./scenarios/plot-axis-support.js";
import { exercisePlotAxisMotion } from "./scenarios/plot-axis-motion.js";
import { exercisePlotAxisGating } from "./scenarios/plot-axis-gating.js";
import { exercisePlotPercentage } from "./scenarios/plot-percentage.js";
import { exercisePlotViewPlacement } from "./scenarios/plot-view-placement.js";
import type { Plot2dContract } from "./support/plot-2d-scene.js";
import type { Plot3dContract } from "./support/plot-3d-scene.js";
import type { ChartContract } from "./scenarios/chart-data-workload.js";

// Each view/viewport owns its fixture, operation budget and cleanup. Large
// completed frames cross the real browser transport before image assertions.
for (const [family, offscreen, transformed] of [
  ["2d", undefined],
  ["dense", undefined],
  ["3d", undefined],
  ["react", undefined],
  ["view", undefined],
  ["view", { width: 427, opposite: false }],
  ["view", { width: 427, opposite: true }],
  ["view", { width: 1280, opposite: false }],
  ["view", { width: 1280, opposite: true }],
  ["axis", undefined, false],
  ["axis", undefined, true],
  ["axis-motion", undefined],
  ["axis-gating", undefined],
  ["percentage", undefined],
] as const) {
  const suffix = offscreen
    ? ` offscreen ${offscreen.width} ${offscreen.opposite ? "opposite" : "front"}`
    : family === "axis"
      ? transformed
        ? " transformed"
        : " identity"
      : "";
  const caseName = `${family}${suffix.replaceAll(" ", "-")}`;
  test(`${family} Plot through native WebSocket/GLES${suffix}`, {
    timeout: 180_000,
  }, async (context) => {
    const workspace = process.cwd();
    const profile = resolve(workspace, "target/gles-host");
    const contract = (await import(
      pathToFileURL(join(profile, "generated.js")).href
    )) as Plot2dContract &
      Plot3dContract &
      ChartContract & {
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
      `plots-${caseName}`,
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
        const record = (label: string, value: unknown) =>
          environment.evidence.record(label, value);
        const capture = createPlotCapture(
          host,
          surface,
          async (label, frame) => {
            await writeFile(
              join(environment.evidence.directory, `${label}.png`),
              encodePng(frame),
            );
          },
          record,
        );
        await environment.execute(`plots-${caseName}`, {}, async () =>
          family === "2d"
            ? exercisePlot2d(host, contract, font, capture, record)
            : family === "dense"
              ? exerciseDenseLines(host, contract, font, capture, record)
              : family === "3d"
                ? exercisePlot3d(host, contract, font, capture)
                : family === "percentage"
                  ? exercisePlotPercentage(
                      host,
                      contract,
                      font,
                      capture,
                      record,
                    )
                  : family === "axis-gating"
                    ? exercisePlotAxisGating(
                        host,
                        contract,
                        font,
                        capture,
                        record,
                      )
                    : family === "axis-motion"
                      ? exercisePlotAxisMotion(
                          host,
                          contract,
                          font,
                          capture,
                          record,
                        )
                      : family === "axis"
                        ? exercisePlotAxisSupport(
                            host,
                            contract,
                            font,
                            capture,
                            record,
                            transformed ?? false,
                          )
                        : family === "view"
                          ? exercisePlotViewPlacement(
                              host,
                              contract,
                              font,
                              capture,
                              record,
                              offscreen,
                            )
                          : reactPlots(host, contract, font, capture, record),
        );
      },
    );
  });

  test(`${family} Plot through worker WASM/WebGL${suffix}`, {
    timeout: 180_000,
  }, async (context) => {
    const workspace = process.cwd();
    const profile = resolve(workspace, "target/browser-build/render");
    await runBrowserEnvironment(
      `plots-${caseName}-webgl`,
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
                rgba: string;
              };
              const pixels = Buffer.from(frame.rgba, "base64");
              if (pixels.length !== frame.width * frame.height * 4)
                throw new Error(
                  "Plot capture RGBA byte count does not match its viewport",
                );
              await writeFile(
                join(environment.evidence.directory, `${frame.label}.png`),
                encodePng({ width: frame.width, height: frame.height, pixels }),
              );
              await environment.evidence.record(label, {
                label: frame.label,
                width: frame.width,
                height: frame.height,
                bytes: pixels.length,
              });
            } else await environment.evidence.record(label, value);
          },
        );
        await environment.execute(`plots-${caseName}-webgl`, {}, () =>
          environment.page.evaluate(
            async ({ urls, family, offscreen, transformed }) => {
              const driver = await import(
                `${urls.origin}/target/plots/plots-driver.js`
              );
              return driver.workerPlots(urls, family, offscreen, transformed);
            },
            { urls: environment.urls, family, offscreen, transformed },
          ),
        );
      },
    );
  });
}
