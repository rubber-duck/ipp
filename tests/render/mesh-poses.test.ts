import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";
import { invoke, writeDataUrl } from "./evidence.js";
import type { run } from "./mesh-poses-fixture.js";

const workspace = resolve(process.cwd());
for (const name of ["render-mesh-poses", "render-expanded"] as const) {
  test(`${name}: corresponding positions match baked geometry through real assets and completed frames`, {
    timeout: 90_000,
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
            `${environment.urls.origin}/dist/tests/render/mesh-poses-fixture.js`,
            "run",
            [environment.urls],
          ),
        );
        assert.equal(result.independentInstances, 2);
        assert.equal(result.contextRestored, true);
        assert.ok(result.comparisons.length >= 10);
      },
    );
  });
}
