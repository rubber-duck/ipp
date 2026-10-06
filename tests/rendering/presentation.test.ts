import test from "node:test";
import { resolve } from "node:path";
import { mkdir, writeFile } from "node:fs/promises";
import { encodePng } from "../harness/images.js";
import { runBrowserEnvironment } from "../harness/browser.js";
import { presentingBuild, withPresentingHost } from "../harness/hosts.js";

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
    scenario: "presentation-webgl",
  },
  {
    title: "the production WebGL render distribution",
    native: false,
    instrumentation: false,
    directory: "target/browser-build/render",
    scenario: "presentation-webgl-production",
  },
  {
    title: "native WebSocket/GLES",
    native: true,
    instrumentation: true,
    directory: "target/gles-host-instrumentation",
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
    const build = presentingBuild(native, {
      directory: distribution.directory,
      instrumentation,
    });
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
    await withPresentingHost(
      "presentation-gles-host",
      build,
      context.signal,
      {
        native,
        operationTimeoutMs: 90_000,
        missingPresentation: "GLES diagnostics URL absent",
      },
      browser,
    );
  });
}
