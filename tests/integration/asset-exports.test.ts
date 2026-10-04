import assert from "node:assert/strict";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { mkdir, writeFile } from "node:fs/promises";
import test from "node:test";
import { runNativeEnvironment } from "./environment.js";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
} from "../browser/environment.js";
import { assetExportRoundTrip } from "./scenarios/asset-exports.js";
import { encodePng } from "../render/retained-gui-images.js";

test("native CPU authorized asset export round trip", {
  timeout: 60000,
}, async (context) => {
  const profile = resolve("target/world-host-build/native");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  await runNativeEnvironment(
    "asset-export-cpu",
    {
      executable: resolve(
        profile,
        process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
      ),
      schemaArtifact: resolve(profile, "contract.bin"),
      workingDirectory: process.cwd(),
      operationTimeoutMs: 45000,
    },
    context.signal,
    async (environment) => {
      const host = await environment.track<
        Parameters<typeof assetExportRoundTrip>[0]
      >(contract.IppHostClient.connectWebSocket(environment.url));
      const peer = await environment.track<
        Parameters<typeof assetExportRoundTrip>[1]
      >(contract.IppHostClient.connectWebSocket(environment.url));
      const result = await environment.execute(
        "authorized CPU export",
        {},
        () =>
          assetExportRoundTrip(host, peer, undefined, {
            graphics: false,
            evidence: {
              record: (value) =>
                environment.evidence.record("asset-export", value),
            },
          }),
      );
      assert.equal(result.exactRgba, true);
    },
  );
});

for (const mode of ["browser", "native"] as const) {
  test(`${mode} GPU authorized asset export round trip`, {
    timeout: 120000,
  }, async (context) => {
    const native = mode === "native";
    const profile = resolve(
      native
        ? "target/gles-host-instrumentation"
        : "target/browser-build/render-instrumentation",
    );
    const build: BrowserBuildConfiguration = {
      name: native ? "gles" : "render-instrumentation",
      generatedModule: resolve(profile, "generated.js"),
      runtimeWasm: resolve(profile, native ? "gles_host" : "runtime.wasm"),
      contractArtifact: resolve(profile, "contract.bin"),
    };
    const browser = async (endpoint?: {
      url: string;
      presentationUrl: string;
    }) =>
      runBrowserEnvironment(
        `asset-export-${mode}`,
        {
          workspace: process.cwd(),
          build,
          operationTimeoutMs: 80000,
          rendering: !native,
        },
        context.signal,
        async (environment) => {
          const captures = resolve(environment.evidence.directory, "captures");
          await mkdir(captures, { recursive: true });
          if (!native)
            await environment.page.exposeFunction(
              "assetExportFenceGate",
              async (
                operation: "hold" | "suspend" | "release" | "resume",
                world: string,
              ) => {
                const worker = environment.page.workers()[0];
                if (!worker) throw new Error("Actual export worker absent");
                return worker.evaluate(
                  async ({ operation, world }) => {
                    const api = (
                      globalThis as unknown as {
                        ippAssetExportTasks: {
                          hold(): void;
                          waiting(): number;
                          suspend(): void;
                          release(): void;
                          resume(): void;
                          tick(world: bigint): bigint;
                        };
                      }
                    ).ippAssetExportTasks;
                    if (operation === "hold") {
                      api.hold();
                      return "";
                    }
                    if (operation === "suspend") {
                      const deadline = performance.now() + 10000;
                      while (api.waiting() === 0) {
                        if (performance.now() > deadline)
                          throw new Error(
                            "GPU staging did not reach the instrumentation gate",
                          );
                        await new Promise<void>((resolve) =>
                          setTimeout(resolve, 2),
                        );
                      }
                      api.suspend();
                      return api.tick(BigInt(world)).toString();
                    }
                    if (operation === "release") {
                      api.release();
                      return "";
                    }
                    const tick = api.tick(BigInt(world)).toString();
                    api.release();
                    api.resume();
                    return tick;
                  },
                  { operation, world },
                );
              },
            );
          await environment.page.exposeFunction(
            "assetExportRecord",
            (value: object) =>
              environment.evidence.record("asset-export", value),
          );
          await environment.page.exposeFunction(
            "assetExportCapture",
            async (
              name: string,
              image: { width: number; height: number; pixels: number[] },
            ) =>
              writeFile(
                resolve(captures, `${name}.png`),
                encodePng({ ...image, pixels: Uint8Array.from(image.pixels) }),
              ),
          );
          const result = await environment.execute(
            "generated-client GPU export and completed image fidelity",
            {},
            () =>
              environment.page.evaluate(
                async ({ urls, endpoint }) => {
                  const scenario = await import(
                    `${urls.origin}/target/multiplex-tests/asset-export-browser.js`
                  );
                  return scenario.probe(urls, endpoint);
                },
                { urls: environment.urls, endpoint },
              ),
          );
          assert.equal(result.exactRgba, true);
          assert.equal(result.reloaded, true);
          assert.equal(result.detached, true);
        },
      );
    if (native)
      await runNativeEnvironment(
        "asset-export-gles",
        {
          executable: build.runtimeWasm,
          schemaArtifact: build.contractArtifact,
          workingDirectory: process.cwd(),
          extraArguments: [
            "--egl-dir",
            process.env.IPP_EGL_LIBRARY_DIR ?? "/lib64",
          ],
          readinessTimeoutMs: 30000,
          operationTimeoutMs: 100000,
        },
        context.signal,
        async (environment) => {
          if (!environment.presentationUrl)
            throw new Error("Native GLES presentation endpoint absent");
          await browser({
            url: environment.url,
            presentationUrl: environment.presentationUrl,
          });
        },
      );
    else await browser();
  });
}
