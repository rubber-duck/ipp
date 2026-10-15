import assert from "node:assert/strict";
import test from "node:test";
import { resolve } from "node:path";
import { runBrowserEnvironment } from "../harness/browser.js";
import { invoke } from "../harness/page-calls.js";

test("composed Camera queries and requester ticks through worker WASM", {
  timeout: 60_000,
}, async (context) => {
  await runBrowserEnvironment(
    "worker composed Camera",
    {
      workspace: resolve(process.cwd()),
      build: {
        name: "render",
        generatedModule: resolve("target/browser-build/render/generated.js"),
        runtimeWasm: resolve("target/browser-build/render/runtime.wasm"),
        contractArtifact: resolve("target/browser-build/render/contract.bin"),
      },
      operationTimeoutMs: 30_000,
    },
    context.signal,
    async (environment) => {
      const result = await invoke(
        environment.page,
        `${environment.urls.origin}/dist/tests/rendering/pages/composed-camera.js`,
        "exercise",
        [
          {
            generatedModuleUrl: environment.urls.generated,
            workerScriptUrl: environment.urls.workerScript,
            wasmUrl: environment.urls.wasm,
          },
        ],
      );
      assert.ok(result);
      await environment.evidence.record("composed-camera", result);
    },
  );
});
