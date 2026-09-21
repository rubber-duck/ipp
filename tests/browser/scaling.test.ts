import { resolve } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "./environment.js";

for (const deep of [false, true])
  test(`worker bounded inspection and restoration: ${deep ? "deep" : "flat"}`, {
    timeout: 90_000,
  }, async (context) => {
    const workspace = resolve(process.cwd());
    const profile = resolve(workspace, "target/world-host-build/wasm");
    const build = {
      name: "world-host" as const,
      generatedModule: resolve(profile, "generated.js"),
      runtimeWasm: resolve(profile, "runtime.wasm"),
      exportWasm: resolve(profile, "export.wasm"),
      contractArtifact: resolve(profile, "contract.bin"),
    };
    await runBrowserEnvironment(
      `worker scaling ${deep}`,
      { workspace, build, mismatchBuild: build, operationTimeoutMs: 60_000 },
      context.signal,
      async (environment) => {
        const result = await environment.execute(
          "worker scaling",
          { deep },
          () =>
            environment.page.evaluate(
              async ({ urls, deep }) => {
                const contract = await import(urls.generated);
                const { scalingAndInspection } = await import(
                  `${urls.origin}/dist/tests/integration/scenarios/scaling.js`
                );
                const host = await contract.IppHostClient.connectWorker(
                  urls.workerScript,
                  urls.wasm,
                  { timeoutMs: 30_000 },
                );
                try {
                  return await scalingAndInspection(host, 1_000, deep);
                } finally {
                  await host.close();
                }
              },
              { urls: environment.urls, deep },
            ),
        );
        environment.evidence.record("scaling_timings", result);
      },
    );
  });
