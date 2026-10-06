import test from "node:test";
import { resolve } from "node:path";
import { mkdir, writeFile } from "node:fs/promises";
import { encodePng } from "../../harness/images.js";
import { runBrowserEnvironment } from "../../harness/browser.js";
import { presentingBuild, withPresentingHost } from "../../harness/hosts.js";
import type { MotionEnvironment, MotionImage } from "./scenarios/motion.js";

declare global {
  interface Window {
    saveMotionCapture(image: MotionImage): Promise<void>;
    recordMotionEvidence(value: object): Promise<void>;
  }
}

for (const mode of ["development", "production", "native"] as const) {
  test(`ordinary skin motion ${mode}`, {
    timeout: 240_000,
  }, async (context) => {
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
        `gui-motion-${mode}`,
        {
          workspace,
          build,
          operationTimeoutMs: 150_000,
          rendering: !native,
        },
        context.signal,
        async (environment) => {
          const captures = resolve(environment.evidence.directory, "captures");
          await mkdir(captures, { recursive: true });
          await environment.page.exposeFunction(
            "saveMotionCapture",
            async (image: MotionImage) => {
              await writeFile(
                resolve(captures, `${image.name}.png`),
                encodePng({ ...image, pixels: Uint8Array.from(image.pixels) }),
              );
              await writeFile(
                resolve(captures, `${image.name}.json`),
                JSON.stringify({ ...image, pixels: undefined }, (_, value) =>
                  typeof value === "bigint" ? value.toString() : value,
                ),
              );
            },
          );
          await environment.page.exposeFunction(
            "recordMotionEvidence",
            (value: object) => environment.evidence.record("motion", value),
          );
          return environment.execute("ordinary-motion", { mode }, () =>
            environment.page.evaluate(
              async ({ urls, endpoint, mode }) => {
                const contract = await import(urls.generated);
                const scenario = await import(
                  `${urls.origin}/target/gui-motion/scenario.js`
                );
                let transport;
                if (endpoint) {
                  transport = scenario.nativePresentationTransport(
                    endpoint.url,
                    endpoint.presentationUrl,
                  );
                } else {
                  const canvas = document.createElement("canvas");
                  canvas.width = 96;
                  canvas.height = 64;
                  document.body.append(canvas);
                  transport = scenario.workerTransport(
                    `${urls.origin}/target/gui-motion/worker-${mode}.js`,
                    urls.wasm,
                    contract.MAX_MESSAGE_BYTES,
                    { canvas: canvas.transferControlToOffscreen() },
                  );
                }
                const host =
                  await contract.IppHostClient.connectTransport(transport);
                // The kit's spinner draws its label in the shared GUI font.
                const font = await (
                  await fetch(
                    `${urls.origin}/target/font-assets/shure-tech-mono.ippf`,
                  )
                ).arrayBuffer();
                const motion: MotionEnvironment = {
                  capture: (image) => window.saveMotionCapture(image),
                  record: (value) => window.recordMotionEvidence(value),
                };
                try {
                  return await scenario.ordinarySkinMotion(
                    host,
                    contract,
                    font,
                    motion,
                  );
                } finally {
                  await host.close();
                }
              },
              { urls: environment.urls, endpoint, mode },
            ),
          );
        },
      );
    }

    await withPresentingHost(
      "gui-motion-gles",
      build,
      context.signal,
      {
        native,
        operationTimeoutMs: 150_000,
        missingPresentation: "GLES presentation endpoint absent",
      },
      browser,
    );
  });
}
