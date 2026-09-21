import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";
import { invoke, writeDataUrl } from "./evidence.js";
import type { run } from "./hierarchy-fixture.js";
for (const name of ["render", "render-expanded"] as const) {
  test(`${name}: hierarchy and terminal aim match independent affine references, picking and restored frames`, {
    timeout: 90000,
  }, async (context) => {
    const workspace = resolve(process.cwd());
    const directory = resolve(workspace, "target/browser-build", name);
    const build = {
      name,
      generatedModule: resolve(directory, "generated.js"),
      runtimeWasm: resolve(directory, "runtime.wasm"),
      exportWasm: resolve(directory, "export.wasm"),
      contractArtifact: resolve(directory, "contract.bin"),
    };
    await runBrowserEnvironment(
      `hierarchy-${name}`,
      {
        workspace,
        build,
        mismatchBuild: build,
        operationTimeoutMs: 60000,
        evidenceParent: resolve(
          workspace,
          "target/integration-artifacts/hierarchy",
        ),
      },
      context.signal,
      async (environment) => {
        await environment.page.exposeFunction(
          "recordHierarchy",
          async (kind: string, value: unknown) => {
            if (kind === "capture") {
              const { label, dataUrl, ...rest } = value as {
                label: string;
                dataUrl: string;
              };
              await writeDataUrl(
                resolve(environment.evidence.directory, `${label}.png`),
                dataUrl,
              );
              await environment.evidence.record(kind, { label, ...rest });
            } else await environment.evidence.record(kind, value);
          },
        );
        const result = await environment.execute("hierarchy frames", {}, () =>
          invoke<Awaited<ReturnType<typeof run>>>(
            environment.page,
            `${environment.urls.origin}/dist/tests/render/hierarchy-fixture.js`,
            "run",
            [environment.urls],
          ),
        );
        assert.equal(result.comparisons, 3);
        assert.equal(result.picking, true);
        assert.equal(result.persistence, true);
      },
    );
  });
}
