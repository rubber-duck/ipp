import test from "node:test";
import assert from "node:assert/strict";
import { resolve } from "node:path";
import { mkdir, writeFile } from "node:fs/promises";
import { parseArgs } from "node:util";
import { runBrowserEnvironment } from "../../harness/browser.js";
import { presentingBuild, withPresentingHost } from "../../harness/hosts.js";
import { encodePng } from "../../harness/images.js";

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
  throw new Error("Native Canvas controller requires --egl-dir");
if (!native && eglDirectory)
  throw new Error("Worker Canvas controller does not use --egl-dir");

test(`React Canvas controller through ${native ? "native WebSocket/GLES" : "worker WASM/WebGL"}`, {
  timeout: 120_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const build = presentingBuild(native, {
    directory: native
      ? "target/gles-host-instrumentation"
      : "target/browser-build/render-instrumentation",
    instrumentation: true,
  });
  async function browser(native?: { url: string; presentationUrl: string }) {
    const result = await runBrowserEnvironment(
      native ? "react-canvas-gles" : "react-canvas-webgl",
      {
        workspace,
        build,
        rendering: !native,
        operationTimeoutMs: 90_000,
      },
      context.signal,
      async (environment) =>
        environment.execute("react-canvas-controller", {}, () =>
          environment.page.evaluate(
            async ({ urls, native }) => {
              const contract = await import(urls.generated);
              const scenario = await import(
                `${urls.origin}/target/canvas-build/controller.js`
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
                return await scenario.canvasController(host);
              } finally {
                await host.close();
              }
            },
            { urls: environment.urls, native },
          ),
        ),
    );
    assert.equal(result.value.images.length, 3);
    const captures = resolve(result.evidenceDirectory, "captures");
    await mkdir(captures);
    for (const image of result.value.images)
      await writeFile(
        resolve(captures, `${image.label}.png`),
        encodePng({ ...image, pixels: Uint8Array.from(image.pixels) }),
      );
  }
  await withPresentingHost(
    "react-canvas-gles-host",
    build,
    context.signal,
    {
      native,
      eglDirectory,
      operationTimeoutMs: 100_000,
      missingPresentation: "GLES presentation diagnostics missing",
    },
    browser,
  );
});
