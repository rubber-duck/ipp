import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../harness/browser.js";
import { invoke } from "../harness/page-calls.js";
import { writeDataUrl } from "../harness/evidence.js";
import type { run } from "./pages/mesh-poses.js";

const workspace = resolve(process.cwd());
const name = "render-instrumentation" as const;
for (const scene of ["unlit", "lit"] as const) {
  test(`${scene}: corresponding positions match baked geometry through real assets and completed frames`, {
    timeout: 90_000,
  }, async (context) => {
    const directory = resolve(workspace, "target/browser-build", name);
    const build = {
      name,
      generatedModule: resolve(directory, "generated.js"),
      runtimeWasm: resolve(directory, "runtime.wasm"),
      contractArtifact: resolve(directory, "contract.bin"),
    };
    await runBrowserEnvironment(
      `mesh-poses-${scene}`,
      {
        workspace,
        build,
        operationTimeoutMs: 60_000,
        evidenceParent: resolve(
          workspace,
          "target/integration-artifacts/mesh-poses",
        ),
      },
      context.signal,
      async (environment) => {
        await environment.page.exposeFunction(
          "recordMeshPose",
          async (kind: string, value: unknown) => {
            if (kind === "capture") {
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
        const result = await environment.execute("mesh poses", {}, () =>
          invoke<Awaited<ReturnType<typeof run>>>(
            environment.page,
            `${environment.urls.origin}/dist/tests/rendering/pages/mesh-poses.js`,
            "run",
            [environment.urls, scene],
          ),
        );
        assert.equal(result.independentInstances, 2);
        assert.equal(result.contextRestored, true);
        assert.ok(result.comparisons.length >= 10);
      },
    );
  });
}
