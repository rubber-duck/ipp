import test from "node:test";
import { resolve } from "node:path";
import { mkdir, writeFile } from "node:fs/promises";
import { encodePng } from "../../render/retained-gui-images.js";
import { runNativeEnvironment } from "../environment.js";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
} from "../../browser/environment.js";
import type { MotionEnvironment, MotionImage } from "./scenario.js";

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
    const directory = resolve(
      native ? "target/gles-host" : "target/browser-build/render",
    );
    const build: BrowserBuildConfiguration = {
      name: native ? "gles" : "render",
      generatedModule: resolve(directory, "generated.js"),
      runtimeWasm: resolve(directory, native ? "gles_host" : "runtime.wasm"),
      contractArtifact: resolve(directory, "contract.bin"),
    };

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

    if (native) {
      await runNativeEnvironment(
        "gui-motion-gles",
        {
          executable: build.runtimeWasm,
          schemaArtifact: build.contractArtifact,
          workingDirectory: workspace,
          extraArguments: [
            "--egl-dir",
            process.env.IPP_EGL_LIBRARY_DIR ?? "/lib64",
          ],
          readinessTimeoutMs: 30_000,
          operationTimeoutMs: 150_000,
        },
        context.signal,
        async (environment) => {
          if (!environment.presentationUrl)
            throw new Error("GLES presentation endpoint absent");
          await browser({
            url: environment.url,
            presentationUrl: environment.presentationUrl,
          });
        },
      );
    } else await browser();
  });
}
