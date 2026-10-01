import test from "node:test";
import { relative, resolve } from "node:path";
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
    stageMotionAsset(bytes: number[]): Promise<void>;
    motionAssetRequested(): Promise<void>;
    releaseMotionAsset(): Promise<void>;
  }
}

for (const mode of ["development", "production", "native"] as const) {
  test(`ordinary skin motion ${mode}`, {
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
      let release!: () => void;
      let requested!: () => void;
      const held = new Promise<void>((resolve) => {
        release = resolve;
      });
      const request = new Promise<void>((resolve) => {
        requested = resolve;
      });
      await runBrowserEnvironment(
        `gui-motion-${mode}`,
        {
          workspace,
          build,
          operationTimeoutMs: 120_000,
          rendering: !native,
          async beforeArtifactResponse(url, signal) {
            if (url.pathname.endsWith("/pending-motion.bin")) {
              requested();
              await Promise.race([
                held,
                new Promise<void>((resolve) =>
                  signal.addEventListener("abort", () => resolve(), {
                    once: true,
                  }),
                ),
              ]);
            }
          },
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
          await environment.page.exposeFunction(
            "stageMotionAsset",
            async (bytes: number[]) =>
              writeFile(
                resolve(environment.evidence.directory, "pending-motion.bin"),
                Uint8Array.from(bytes),
              ),
          );
          await environment.page.exposeFunction(
            "motionAssetRequested",
            () => request,
          );
          await environment.page.exposeFunction("releaseMotionAsset", () =>
            release(),
          );
          const pendingSource = `${environment.urls.origin}/${relative(workspace, environment.evidence.directory)}/pending-motion.bin`;
          return environment.execute("ordinary-motion", { mode }, () =>
            environment.page.evaluate(
              async ({ urls, endpoint, mode, pendingSource }) => {
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
                const motion: MotionEnvironment = {
                  capture: (image) => window.saveMotionCapture(image),
                  record: (value) => window.recordMotionEvidence(value),
                  ...(endpoint
                    ? {}
                    : {
                        pending: {
                          source: pendingSource,
                          stage: (bytes) => window.stageMotionAsset(bytes),
                          requested: () => window.motionAssetRequested(),
                          release: () => window.releaseMotionAsset(),
                        },
                      }),
                };
                try {
                  return await scenario.ordinarySkinMotion(
                    host,
                    contract,
                    motion,
                  );
                } finally {
                  await host.close();
                }
              },
              { urls: environment.urls, endpoint, mode, pendingSource },
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
