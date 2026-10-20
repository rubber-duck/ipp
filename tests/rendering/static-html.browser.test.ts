import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../harness/browser.js";
import { invoke } from "../harness/page-calls.js";
import { encodePng } from "../harness/images.js";
import { staticHtmlScenario } from "./scenarios/static-html.js";

for (const [deviceScaleFactor, zoom] of [
  [1, 1],
  [1.25, 1],
  [2, 1],
  [1, 1.1],
  [1, 1.25],
  [2, 2],
] as const)
  test(`static HTML at ${deviceScaleFactor}x display and ${zoom}x CSS zoom uses the real Host`, {
    timeout: 60_000,
  }, async (context) => {
    const root = resolve("target/browser-build/render-instrumentation");
    await runBrowserEnvironment(
      `static-html-${deviceScaleFactor}x-zoom-${zoom}`,
      {
        workspace: process.cwd(),
        build: {
          name: "render-instrumentation",
          generatedModule: join(root, "generated.js"),
          runtimeWasm: join(root, "runtime.wasm"),
          contractArtifact: join(root, "contract.bin"),
        },
        operationTimeoutMs: 15_000,
        deviceScaleFactor,
        evidenceParent: resolve("target/html-canvas-evidence"),
      },
      context.signal,
      async (env) => {
        const module = `${env.urls.origin}/target/html-canvas/static-html.js`;
        const call = <T>(name: string, ...args: unknown[]) =>
          invoke<T>(env.page, module, name, args);
        try {
          await call("create", {
            generated: env.urls.generated,
            worker: env.urls.workerScript,
            wasm: env.urls.wasm,
            zoom,
          });
          await staticHtmlScenario({
            call,
            save: async (label, frame) =>
              writeFile(
                join(env.evidence.directory, `${label}.png`),
                encodePng({ ...frame, pixels: Uint8Array.from(frame.pixels) }),
              ),
          });
        } finally {
          assert.equal(await call("close"), 0);
        }
      },
    );
  });
