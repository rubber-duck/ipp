import assert from "node:assert/strict";
import { join, resolve } from "node:path";
import test from "node:test";
import { writeDataUrl } from "../harness/evidence.js";
import { runBrowserEnvironment } from "../harness/browser.js";

const profile = resolve("target/browser-build/render-instrumentation");
test("real worker mounts transferred and sealed external buffers through scoped IO", {
  timeout: 30000,
}, async (context) => {
  await runBrowserEnvironment(
    "generated-buffer-io",
    {
      workspace: process.cwd(),
      crossOriginIsolation: true,
      build: {
        name: "render-instrumentation",
        generatedModule: resolve(profile, "generated.js"),
        runtimeWasm: resolve(profile, "runtime.wasm"),
        contractArtifact: resolve(profile, "contract.bin"),
      },
      operationTimeoutMs: 15000,
    },
    context.signal,
    async (environment) => {
      const moduleUrl = `${environment.urls.origin}/target/multiplex-tests/generated-buffers.js`;
      const result = await environment.execute(
        "mount, decode, account and release generated source ownership",
        {},
        () =>
          environment.page.evaluate(
            async ({ moduleUrl, urls }) => {
              const scenario = (await import(
                moduleUrl
              )) as typeof import("./pages/generated-buffers.js");
              return scenario.generatedBuffers(urls);
            },
            { moduleUrl, urls: environment.urls },
          ),
      );
      await writeDataUrl(
        join(environment.evidence.directory, "generated-textures-capture.png"),
        result.capture.dataUrl,
      );
      await environment.evidence.writeJson("generated-textures-frame.json", {
        frame: result.capture.frame,
        sourceTick: result.capture.sourceTick,
        patches: result.capture.patches,
      });
      assert.equal(result.capture.frame.drawCalls, 2);
      assert.equal(result.capture.frame.triangles, 4);
      assert.equal(result.capture.frame.failedDrawCalls, 0);
      // Expected content is independent of the browser fixture's payload writer.
      const expected = [
        [
          [255, 0, 0, 255],
          [0, 255, 0, 255],
          [0, 0, 255, 255],
          [255, 255, 0, 255],
        ],
        [
          [0, 255, 255, 255],
          [255, 0, 255, 255],
          [255, 255, 255, 255],
          [0, 0, 0, 255],
        ],
      ];
      for (const patch of result.capture.patches)
        for (const pixel of patch.pixels)
          for (let channel = 0; channel < 4; channel++)
            assert.ok(
              Math.abs(
                pixel[channel]! -
                  expected[patch.source]![patch.quadrant]![channel]!,
              ) <= 3,
              `Source ${patch.source}, quadrant ${patch.quadrant}, channel ${channel}: ${pixel}`,
            );
      assert.equal(result.crossingBytes, 2 * result.length);
      assert.equal(result.releasedBackingBytes, 0);
      assert.equal(environment.page.workers().length, 0);
    },
  );
});
