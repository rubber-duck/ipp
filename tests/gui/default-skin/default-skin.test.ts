import test from "node:test";
import { resolve } from "node:path";
import { mkdir, writeFile } from "node:fs/promises";
import { encodePng } from "../../harness/images.js";
import { runBrowserEnvironment } from "../../harness/browser.js";
import { presentingBuild, withPresentingHost } from "../../harness/hosts.js";
import type { DefaultSkinImage } from "./scenarios/default-skin.js";

declare global {
  interface Window {
    saveDefaultSkinCapture(image: DefaultSkinImage): Promise<void>;
    recordDefaultSkinEvidence(value: object): Promise<void>;
  }
}

/** The shared GUI font built by the `font-assets` product. */
const FONT = "target/font-assets/shure-tech-mono.ippf";

for (const mode of ["worker", "native"] as const) {
  test(`default GUI skin ${mode}`, { timeout: 300_000 }, async (context) => {
    const native = mode === "native";
    const workspace = resolve(process.cwd());
    const build = presentingBuild(native, {
      directory: native ? "target/gles-host" : "target/browser-build/render",
    });

    async function browser(endpoint?: {
      url: string;
      presentationUrl: string;
    }) {
      await runBrowserEnvironment(
        `gui-default-skin-${mode}`,
        {
          workspace,
          build,
          operationTimeoutMs: 240_000,
          rendering: !native,
        },
        context.signal,
        async (environment) => {
          const captures = resolve(environment.evidence.directory, "captures");
          await mkdir(captures, { recursive: true });
          await environment.page.exposeFunction(
            "saveDefaultSkinCapture",
            async (image: DefaultSkinImage) =>
              writeFile(
                resolve(captures, `${image.name}.png`),
                encodePng({
                  width: image.width,
                  height: image.height,
                  pixels: Uint8Array.from(Buffer.from(image.rgba, "base64")),
                }),
              ),
          );
          await environment.page.exposeFunction(
            "recordDefaultSkinEvidence",
            (value: object) =>
              environment.evidence.record("default-skin", value),
          );
          return environment.execute("default-skin", { mode }, () =>
            environment.page.evaluate(
              async ({ urls, endpoint, font }) => {
                const contract = await import(urls.generated);
                // The skin lab's tokens come from the runtime under test.
                Object.assign(globalThis, { ippHostContract: contract });
                const scenario = await import(
                  `${urls.origin}/target/gui-default-skin/scenario.js`
                );
                const fontBytes = new Uint8Array(
                  await (await fetch(`${urls.origin}/${font}`)).arrayBuffer(),
                );
                let transport;
                if (endpoint) {
                  transport = scenario.nativePresentationTransport(
                    endpoint.url,
                    endpoint.presentationUrl,
                  );
                } else {
                  const canvas = document.createElement("canvas");
                  canvas.width = 2048;
                  canvas.height = 256;
                  document.body.append(canvas);
                  transport = scenario.workerTransport(
                    `${urls.origin}/target/gui-default-skin/worker.js`,
                    urls.wasm,
                    contract.MAX_MESSAGE_BYTES,
                    { canvas: canvas.transferControlToOffscreen() },
                  );
                }
                const host =
                  await contract.IppHostClient.connectTransport(transport);
                try {
                  return await scenario.defaultSkin(host, contract, fontBytes, {
                    capture: (image: DefaultSkinImage) =>
                      window.saveDefaultSkinCapture(image),
                    record: (value: object) =>
                      window.recordDefaultSkinEvidence(value),
                  });
                } finally {
                  await host.close();
                }
              },
              { urls: environment.urls, endpoint, font: FONT },
            ),
          );
        },
      );
    }

    await withPresentingHost(
      "gui-default-skin-gles",
      build,
      context.signal,
      {
        native,
        operationTimeoutMs: 270_000,
        missingPresentation: "GLES presentation endpoint absent",
      },
      browser,
    );
  });
}
