import test from "node:test";
import { resolve } from "node:path";
import { mkdir, writeFile } from "node:fs/promises";
import { encodePng } from "../render/retained-gui-images.js";
import { runNativeEnvironment } from "./environment.js";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
} from "../browser/environment.js";

/**
 * Each presenting distribution. Only instrumentation builds honour the testing
 * controls; the production render build refuses them at the call.
 */
const presentations = [
  {
    title: "worker WASM/WebGL",
    native: false,
    instrumentation: true,
    directory: "target/browser-build/render-instrumentation",
    name: "render-instrumentation",
    scenario: "presentation-webgl",
  },
  {
    title: "the production WebGL render distribution",
    native: false,
    instrumentation: false,
    directory: "target/browser-build/render",
    name: "render",
    scenario: "presentation-webgl-production",
  },
  {
    title: "native WebSocket/GLES",
    native: true,
    instrumentation: true,
    directory: "target/gles-host-instrumentation",
    name: "gles",
    scenario: "presentation-gles",
  },
] as const;

for (const {
  title,
  native,
  instrumentation,
  ...distribution
} of presentations) {
  test(`explicit physical presentation through ${title}`, {
    timeout: 120_000,
  }, async (context) => {
    const workspace = resolve(process.cwd());
    const directory = resolve(distribution.directory);
    const build: BrowserBuildConfiguration = {
      name: distribution.name,
      generatedModule: resolve(directory, "generated.js"),
      runtimeWasm: resolve(directory, native ? "gles_host" : "runtime.wasm"),
      contractArtifact: resolve(directory, "contract.bin"),
    };
    async function browser(urls?: { url: string; presentationUrl: string }) {
      const result = await runBrowserEnvironment(
        distribution.scenario,
        {
          workspace,
          build,
          operationTimeoutMs: 60_000,
          rendering: !native,
        },
        context.signal,
        async (environment) =>
          environment.execute("explicit-presentation", {}, () =>
            environment.page.evaluate(
              async ({ urls, native, instrumentation }) => {
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
                    instrumentation,
                  );
                  const resize = await scenario.retainedResize(
                    host,
                    await (
                      await fetch(
                        `${urls.origin}/target/font-assets/shure-tech-mono.ippf`,
                      )
                    ).arrayBuffer(),
                  );
                  const connectionLifetime = native
                    ? await scenario.connectionPresentationLifetime(() =>
                        contract.IppHostClient.connectWebSocket(native.url),
                      )
                    : null;
                  return { ...presentation, resize, connectionLifetime };
                } finally {
                  await host.close();
                }
              },
              { urls: environment.urls, native: urls, instrumentation },
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
      for (const [index, image] of result.value.resize.images.entries()) {
        await writeFile(
          resolve(images, `resize-${index}.png`),
          encodePng({ ...image, pixels: Uint8Array.from(image.pixels) }),
        );
      }
      await writeFile(
        resolve(result.evidenceDirectory, "retained-resize.json"),
        `${JSON.stringify({ ...result.value.resize, images: undefined }, (_, value) => (typeof value === "bigint" ? value.toString() : value), 2)}\n`,
      );
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
