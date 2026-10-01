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
  },
});
if (values.backend !== "webgl" && values.backend !== "gles")
  throw new Error("Select --backend webgl or gles");
const native = values.backend === "gles";
const eglDirectory = values["egl-dir"];
if (native && !eglDirectory)
  throw new Error("Native GUI authoring requires --egl-dir");
if (!native && eglDirectory)
  throw new Error("Worker GUI authoring does not use --egl-dir");

for (const variant of native ? ["production"] : ["development", "production"]) {
  test(`React GUI authoring through ${native ? "native WebSocket/GLES" : "worker WASM/WebGL"} ${variant}`, {
    timeout: 120_000,
  }, async (context) => {
    const workspace = resolve(process.cwd());
    const directory = resolve(
      native ? "target/gles-host" : "target/browser-build/render",
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
          operationTimeoutMs: 90_000,
        },
        context.signal,
        async (environment) =>
          environment.execute("react-gui-authoring-paint", {}, () =>
            environment.page.evaluate(
              async ({ urls, native, variant }) => {
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
                  return await scenario.guiPaint(host, contract, {
                    font: await load("target/font-assets/shure-tech-mono.ippf"),
                    drawing: await load("target/surface-assets/icon.ippd"),
                    bitmap: await load("target/surface-assets/badge.ippt"),
                  });
                } finally {
                  await host.close();
                }
              },
              { urls: environment.urls, native, variant },
            ),
          ),
      );
      const captures = resolve(result.evidenceDirectory, "captures");
      await mkdir(captures);
      for (const image of result.value.images)
        await writeFile(
          resolve(captures, `${image.label}.png`),
          encodePng({ ...image, pixels: Uint8Array.from(image.pixels) }),
        );
      assert.equal(result.value.failure, null);
      assert.equal(result.value.images.length, 9);
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
