import test from "node:test";
import assert from "node:assert/strict";
import { resolve } from "node:path";
import { mkdir, writeFile } from "node:fs/promises";
import { parseArgs } from "node:util";
import { runNativeEnvironment } from "../integration/environment.js";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
} from "../browser/environment.js";
import { encodePng } from "../render/retained-gui-images.js";

const { values } = parseArgs({
  options: {
    backend: { type: "string" },
    "egl-dir": { type: "string" },
    projected: { type: "string" },
  },
});
if (values.backend !== "webgl" && values.backend !== "gles")
  throw new Error("Select --backend webgl or gles");
const native = values.backend === "gles";
const projected = values.projected;
if (
  projected !== undefined &&
  projected !== "advanced" &&
  projected !== "recovery"
)
  throw new Error("Select --projected advanced or recovery");
const instrumentation = projected === "recovery";
const eglDirectory = values["egl-dir"];
if (native && !eglDirectory)
  throw new Error("Native GUI authoring requires --egl-dir");
if (!native && eglDirectory)
  throw new Error("Worker GUI authoring does not use --egl-dir");

for (const variant of native || instrumentation
  ? ["production"]
  : ["development", "production"]) {
  test(`React GUI authoring through ${native ? "native WebSocket/GLES" : "worker WASM/WebGL"} ${variant}${projected ? ` projected ${projected}` : ""}`, {
    timeout: 240_000,
  }, async (context) => {
    const workspace = resolve(process.cwd());
    const directory = resolve(
      native
        ? instrumentation
          ? "target/gles-host-instrumentation"
          : "target/gles-host"
        : instrumentation
          ? "target/browser-build/render-instrumentation"
          : "target/browser-build/render",
    );
    const build: BrowserBuildConfiguration = {
      name: native ? "gles" : "render",
      generatedModule: resolve(directory, "generated.js"),
      runtimeWasm: resolve(directory, native ? "gles_host" : "runtime.wasm"),
      contractArtifact: resolve(directory, "contract.bin"),
    };
    async function browser(native?: { url: string; presentationUrl: string }) {
      const result = await runBrowserEnvironment(
        `${native ? "react-gui-gles" : "react-gui-webgl"}-${variant}`,
        {
          workspace,
          build,
          rendering: !native,
          operationTimeoutMs: 210_000,
        },
        context.signal,
        async (environment) =>
          environment.execute("react-gui-authoring-paint", {}, () =>
            environment.page.evaluate(
              async ({ urls, native, variant, projected }) => {
                const contract = await import(urls.generated);
                const scenario = await import(
                  `${urls.origin}/target/react-gui-authoring/paint-${variant}.js`
                );
                const transport = native
                  ? scenario.nativePresentationTransport(
                      native.url,
                      native.presentationUrl,
                    )
                  : scenario.workerTransport(
                      urls.workerScript,
                      urls.wasm,
                      contract.MAX_MESSAGE_BYTES,
                      { canvas: new OffscreenCanvas(96, 64) },
                    );
                const host =
                  await contract.IppHostClient.connectTransport(transport);
                try {
                  const load = async (path: string) =>
                    new Uint8Array(
                      await (
                        await fetch(`${urls.origin}/${path}`)
                      ).arrayBuffer(),
                    );
                  const assets = {
                    font: await load("target/font-assets/shure-tech-mono.ippf"),
                    drawing: await load("target/surface-assets/icon.ippd"),
                    bitmap: await load("target/surface-assets/badge.ippt"),
                  };
                  if (projected)
                    return {
                      advanced: await scenario.guiProjectedAdvanced(
                        host,
                        contract,
                        assets,
                        projected === "recovery",
                      ),
                    };
                  const paint = await scenario.guiPaint(host, contract, assets);
                  const layers = paint.failure
                    ? null
                    : await scenario.guiLayers(host, contract);
                  const overlays = layers?.failure
                    ? null
                    : await scenario.guiOverlays(host, contract);
                  const projectedResult = overlays?.failure
                    ? null
                    : await scenario.guiProjectedSurfaces(
                        host,
                        contract,
                        assets,
                      );
                  return {
                    ...paint,
                    layers,
                    overlays,
                    projected: projectedResult,
                  };
                } finally {
                  await host.close();
                }
              },
              { urls: environment.urls, native, variant, projected },
            ),
          ),
      );
      const captures = resolve(result.evidenceDirectory, "captures");
      await mkdir(captures);
      for (const image of [
        ...(result.value.images ?? []),
        ...(result.value.advanced?.images ?? []),
        ...(result.value.advanced?.comparisons ?? []),
        ...(result.value.layers?.images ?? []),
        ...(result.value.overlays?.images ?? []),
        ...(result.value.projected?.images ?? []),
      ])
        await writeFile(
          resolve(captures, `${image.label}.png`),
          encodePng({ ...image, pixels: Uint8Array.from(image.pixels) }),
        );
      if (result.value.advanced) {
        await writeFile(
          resolve(captures, "advanced-observations.json"),
          `${JSON.stringify(result.value.advanced.observations, (_, value) => (typeof value === "bigint" ? value.toString() : value), 2)}\n`,
        );
        assert.equal(result.value.advanced.failure, null);
        assert.equal(
          result.value.advanced.images.length,
          instrumentation ? 6 : 10,
        );
        return;
      }
      if (result.value.colourErrors)
        await writeFile(
          resolve(captures, "colour-fill-errors.json"),
          `${JSON.stringify(result.value.colourErrors, null, 2)}\n`,
        );
      assert.equal(result.value.failure, null);
      assert.equal(result.value.images.length, 13);
      assert.equal(result.value.layers?.failure, null);
      assert.equal(result.value.layers?.images.length, 8);
      assert.equal(result.value.overlays?.failure, null);
      assert.equal(result.value.overlays?.images.length, 5);
      assert.equal(result.value.projected?.failure, null);
      assert.equal(result.value.projected?.images.length, 7);
    }
    if (native)
      await runNativeEnvironment(
        "react-gui-gles-host",
        {
          executable: build.runtimeWasm,
          schemaArtifact: build.contractArtifact,
          workingDirectory: workspace,
          extraArguments: ["--egl-dir", eglDirectory!],
          readinessTimeoutMs: 30_000,
          operationTimeoutMs: 100_000,
        },
        context.signal,
        async (environment) => {
          if (!environment.presentationUrl)
            throw new Error("GLES presentation diagnostics missing");
          await browser({
            url: environment.url,
            presentationUrl: environment.presentationUrl,
          });
        },
      );
    else await browser();
  });
}
