import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";
import { invoke, writeDataUrl } from "./evidence.js";
import { requireVisible } from "./image-assertions.js";
import type { run } from "./animation-fixture.js";

const workspace = resolve(process.cwd());
for (const name of ["headless", "render"] as const) {
  test(`${name} uses real clip assets and multi-entity controller playback`, {
    timeout: 60_000,
  }, async (context) => {
    const directory = resolve(workspace, "target/browser-build", name);
    const build = {
      name,
      generatedModule: resolve(directory, "generated.js"),
      runtimeWasm: resolve(directory, "runtime.wasm"),
      exportWasm: resolve(directory, "export.wasm"),
      contractArtifact: resolve(directory, "contract.bin"),
    };
    await runBrowserEnvironment(
      name,
      {
        workspace,
        build,
        mismatchBuild: build,
        operationTimeoutMs: 25_000,
        closeTimeoutMs: 5_000,
        evidenceParent: resolve(
          workspace,
          "target/integration-artifacts/animation",
        ),
      },
      context.signal,
      async (environment) => {
        await environment.page.exposeFunction(
          "recordAnimation",
          async (kind: string, value: unknown) => {
            if (kind === "animation.capture") {
              const { label, dataUrl, ...metadata } = value as {
                label: string;
                dataUrl: string;
              };
              await writeDataUrl(
                resolve(environment.evidence.directory, `${label}.png`),
                dataUrl,
              );
              await environment.evidence.record(kind, { label, ...metadata });
            } else await environment.evidence.record(kind, value);
          },
        );
        const result = await environment.execute(
          "property animation",
          { rendering: name === "render" },
          () =>
            invoke<Awaited<ReturnType<typeof run>>>(
              environment.page,
              `${environment.urls.origin}/dist/tests/render/animation-fixture.js`,
              "run",
              [environment.urls, name === "render"],
            ),
        );
        if (name === "headless") {
          assert.equal(result.contract?.independentControllers, 2);
          assert.equal(result.contract?.synchronizedTargets, 2);
          assert.equal(result.contract?.inheritedOriginal, 88);
          assert.equal(result.contract?.signedPlayback, true);
          return;
        }
        const [left, middle, right, paused, stopped] = result.captures;
        for (const frame of result.captures) {
          requireVisible(frame.summary, frame.label);
          assert.equal(frame.metadata.drawCalls, 1);
          assert.equal(frame.metadata.triangles, 12);
        }
        assert.ok(Math.abs(left!.summary.centroidX! - 99.5) < 2);
        assert.ok(
          Math.abs(middle!.summary.centroidX! - 159.5) < 2,
          "Bézier time coordinate must place its midpoint at the center",
        );
        assert.ok(Math.abs(right!.summary.centroidX! - 219.5) < 2);
        assert.ok(
          left!.summary.meanRgb[0] > 240 && left!.summary.meanRgb[1] < 10,
        );
        assert.ok(
          right!.summary.meanRgb[1] > 240 && right!.summary.meanRgb[0] < 10,
        );
        assert.deepEqual(paused!.summary, right!.summary);
        assert.ok(
          stopped!.summary.meanRgb[2] > 240 && stopped!.summary.meanRgb[0] < 10,
        );
        assert.ok(Math.abs(stopped!.summary.centroidX! - 159.5) < 2);
        assert.ok(result.differences[0]!.changedFraction > 0.04);
        assert.equal(result.differences[1]!.changedPixels, 0);
      },
    );
  });
}
