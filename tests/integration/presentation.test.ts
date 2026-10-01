import test from "node:test";
import { resolve } from "node:path";
import { mkdir, writeFile } from "node:fs/promises";
import { encodePng } from "../render/retained-gui-images.js";
import { runNativeEnvironment } from "./environment.js";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
} from "../browser/environment.js";

for (const native of [false, true]) {
  test(`explicit physical presentation through ${native ? "native WebSocket/GLES" : "worker WASM/WebGL"}`, {
    timeout: 120_000,
  }, async (context) => {
    const workspace = resolve(process.cwd());
    const directory = resolve(
      native
        ? "target/gles-host/gles-surfaces"
        : "target/browser-build/render-surfaces",
    );
    const build: BrowserBuildConfiguration = {
      name: native ? "gles-surfaces" : "render-surfaces",
      generatedModule: resolve(directory, "generated.js"),
      runtimeWasm: resolve(directory, native ? "gles_host" : "runtime.wasm"),
      exportWasm: resolve(directory, native ? "contract.bin" : "export.wasm"),
      contractArtifact: resolve(directory, "contract.bin"),
    };
    async function browser(urls?: { url: string; presentationUrl: string }) {
      const result = await runBrowserEnvironment(
        native ? "presentation-gles" : "presentation-webgl",
        {
          workspace,
          build,
          mismatchBuild: build,
          operationTimeoutMs: 60_000,
          rendering: !native,
        },
        context.signal,
        async (environment) =>
          environment.execute("explicit-presentation", {}, () =>
            environment.page.evaluate(
              async ({ urls, native }) => {
                const contract = await import(urls.generated);
                const scenario = await import(
                  `${urls.origin}/target/multiplex-tests/presentation.js`
                );
                let transport;
                if (native) {
                  transport = scenario.nativePresentationTransport(
                    native.url,
                    native.presentationUrl,
                  );
                } else {
                  const canvas = document.createElement("canvas");
                  canvas.width = 96;
                  canvas.height = 64;
                  document.body.append(canvas);
                  transport = scenario.workerTransport(
                    urls.workerScript,
                    urls.wasm,
                    contract.MAX_MESSAGE_BYTES,
                    { canvas: canvas.transferControlToOffscreen() },
                  );
                }
                const controlled = scenario.presentationTransport(
                  contract.WIRE,
                  transport,
                );
                const host = await contract.IppHostClient.connectTransport(
                  controlled.transport,
                );
                try {
                  const presentation = await scenario.explicitPresentation(
                    host,
                    controlled.probe,
                  );
                  const connectionLifetime = native
                    ? await scenario.connectionPresentationLifetime(() =>
                        contract.IppHostClient.connectWebSocket(native.url),
                      )
                    : null;
                  return { ...presentation, connectionLifetime };
                } finally {
                  await host.close();
                }
              },
              { urls: environment.urls, native: urls },
            ),
          ),
      );
      const images = resolve(result.evidenceDirectory, "captures");
      await mkdir(images, { recursive: true });
      for (const [index, image] of result.value.images.entries()) {
        await writeFile(
          resolve(images, `${index}.png`),
          encodePng({ ...image, pixels: Uint8Array.from(image.pixels) }),
        );
      }
      return result;
    }
    if (native) {
      await runNativeEnvironment(
        "presentation-gles-host",
        {
          executable: build.runtimeWasm,
          schemaArtifact: build.contractArtifact,
          workingDirectory: workspace,
          extraArguments: [
            "--egl-dir",
            process.env.IPP_EGL_LIBRARY_DIR ?? "/lib64",
          ],
          readinessTimeoutMs: 30_000,
          operationTimeoutMs: 90_000,
        },
        context.signal,
        async (environment) => {
          if (!environment.presentationUrl)
            throw new Error("GLES diagnostics URL absent");
          await browser({
            url: environment.url,
            presentationUrl: environment.presentationUrl,
          });
        },
      );
    } else await browser();
  });
}
