import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";

const profile = resolve("target/browser-build/render-instrumentation");

test("actual worker keeps peer bulk/control responsive while IO waits and delivers explicit severe-pressure revocation", {
  timeout: 30_000,
}, async (context) => {
  await runBrowserEnvironment(
    "bulk-read-worker",
    {
      workspace: process.cwd(),
      build: {
        name: "render-instrumentation",
        generatedModule: resolve(profile, "generated.js"),
        runtimeWasm: resolve(profile, "runtime.wasm"),
        contractArtifact: resolve(profile, "contract.bin"),
      },
      operationTimeoutMs: 8_000,
    },
    context.signal,
    async (environment) => {
      const result = await environment.execute(
        "pending IO and pressure revocation through actual transport",
        {},
        () =>
          environment.page.evaluate(async (urls) => {
            const scenario = await import(
              `${urls.origin}/target/multiplex-tests/bulk-read-worker.js`
            );
            return scenario.probe(urls);
          }, environment.urls),
      );
      assert.ok(result.bytes > 0);
      assert.equal(result.pendingFailed, true);
      assert.equal(result.noticeReceived, true);
      assert.equal(result.peerResponsive, true);
    },
  );
});
