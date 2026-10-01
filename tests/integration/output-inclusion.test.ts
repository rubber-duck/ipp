import test from "node:test";
import { resolve } from "node:path";
import { mkdir, writeFile } from "node:fs/promises";
import { encodePng } from "../render/retained-gui-images.js";
import { runNativeEnvironment } from "./environment.js";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
} from "../browser/environment.js";

declare global {
  interface Window {
    recordInclusion(value: object): Promise<void>;
    saveInclusion(
      name: string,
      image: { width: number; height: number; pixels: number[] },
    ): Promise<void>;
  }
}

for (const mode of ["development", "production", "native"] as const) {
  test(`composed output inclusion ${mode}`, {
    timeout: 180_000,
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
      return runBrowserEnvironment(
        `output-inclusion-${mode}`,
        {
          workspace,
          build,
          operationTimeoutMs: 120_000,
          rendering: !native,
        },
        context.signal,
        async (environment) => {
          const captures = resolve(environment.evidence.directory, "captures");
          await mkdir(captures, { recursive: true });
          await environment.page.exposeFunction(
            "recordInclusion",
            (value: object) => environment.evidence.record("inclusion", value),
          );
          await environment.page.exposeFunction(
            "saveInclusion",
            async (
              name: string,
              image: { width: number; height: number; pixels: number[] },
            ) => {
              await writeFile(
                resolve(captures, `${name}.png`),
                encodePng({ ...image, pixels: Uint8Array.from(image.pixels) }),
              );
            },
          );
          return environment.execute("composed-inclusion", { mode }, () =>
            environment.page.evaluate(
              async ({ urls, endpoint, mode }) => {
                const contract = await import(urls.generated);
                const scenario = await import(
                  `${urls.origin}/target/multiplex-tests/output-inclusion.js`
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
                    `${urls.origin}/target/multiplex-tests/presentation-worker-${mode}.js`,
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
                  return await scenario.composedOutputInclusion(host, {
                    record: (value: object) => window.recordInclusion(value),
                    capture: (
                      name: string,
                      image: {
                        view: {
                          binding: {
                            viewport: { width: number; height: number };
                          };
                        };
                        pixels: ArrayBuffer;
                      },
                    ) =>
                      window.saveInclusion(name, {
                        ...image.view.binding.viewport,
                        pixels: [...new Uint8Array(image.pixels)],
                      }),
                  });
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
        "output-inclusion-gles-host",
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
