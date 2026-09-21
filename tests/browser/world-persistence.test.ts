import { resolve } from "node:path";
import test from "node:test";
const worldHostCases = [
  {
    name: "named Worlds preserve authored captures and session boundaries",
    method: "namedWorldPersistence",
  },
  {
    name: "World metadata grows and restores beyond the former byte ceiling",
    method: "worldMetadataGrowth",
  },
];
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
} from "./environment.js";

for (const scenario of worldHostCases) {
  test(`browser ${scenario.name}`, { timeout: 30_000 }, async (context) => {
    const workspace = resolve(process.cwd());
    const profile = resolve(workspace, "target/world-host-build/wasm");
    const build: BrowserBuildConfiguration = {
      name: "world-host",
      generatedModule: resolve(profile, "generated.js"),
      runtimeWasm: resolve(profile, "runtime.wasm"),
      exportWasm: resolve(profile, "export.wasm"),
      contractArtifact: resolve(profile, "contract.bin"),
    };
    await runBrowserEnvironment(
      `browser ${scenario.name}`,
      {
        workspace,
        build,
        mismatchBuild: build,
        operationTimeoutMs: 20_000,
      },
      context.signal,
      async (environment) => {
        await environment.page.exposeFunction(
          "recordWorldHost",
          (kind: string, value: unknown) =>
            environment.evidence.record(kind, value),
        );
        return environment.execute(scenario.name, {}, () =>
          environment.page.evaluate(
            async ({ urls, method }) => {
              const contract = await import(urls.generated);
              const scenarios = await import(
                `${urls.origin}/dist/tests/integration/scenarios/world-persistence.js`
              );
              const client = await contract.IppHostClient.connectWorker(
                urls.workerScript,
                urls.wasm,
              );
              try {
                return await scenarios[method](client);
              } finally {
                await client.close();
              }
            },
            { urls: environment.urls, method: scenario.method },
          ),
        );
      },
    );
  });
}
