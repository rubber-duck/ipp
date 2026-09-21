import { resolve } from "node:path";
import test from "node:test";
import { worldHostCases } from "../integration/world-host-cases.js";
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
            async ({ urls, name }) => {
              const contract = await import(urls.generated);
              const { worldHostCases: cases } = await import(
                `${urls.origin}/dist/tests/integration/world-host-cases.js`
              );
              const scenario = cases.find(
                (candidate: { name: string }) => candidate.name === name,
              );
              if (!scenario)
                throw new Error(`Missing scene host scenario: ${name}`);
              const client = await contract.IppClient.connectWorker(
                urls.workerScript,
                urls.wasm,
              );
              try {
                const record = (
                  globalThis as unknown as {
                    recordWorldHost(
                      kind: string,
                      value: unknown,
                    ): Promise<void>;
                  }
                ).recordWorldHost;
                return await scenario.run(
                  client,
                  contract,
                  (kind: string, value: unknown) =>
                    record(
                      kind,
                      JSON.parse(
                        JSON.stringify(value, (_key, item: unknown) =>
                          typeof item === "bigint"
                            ? { $bigint: item.toString() }
                            : item,
                        ),
                      ),
                    ),
                );
              } finally {
                await client.close();
              }
            },
            { urls: environment.urls, name: scenario.name },
          ),
        );
      },
    );
  });
}
