import { relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
} from "./environment.js";

test("browser asset rejection recovery", {
  timeout: 30_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const scenarioPath = relative(
    workspace,
    fileURLToPath(
      new URL("../integration/scenarios/asset-rejection.js", import.meta.url),
    ),
  )
    .split(sep)
    .join("/");
  const profile = resolve(workspace, "target/world-host-build/wasm");
  const build: BrowserBuildConfiguration = {
    name: "world-host",
    generatedModule: resolve(profile, "generated.js"),
    runtimeWasm: resolve(profile, "runtime.wasm"),
    contractArtifact: resolve(profile, "contract.bin"),
  };
  await runBrowserEnvironment(
    "browser asset rejection recovery",
    {
      workspace,
      build,
      operationTimeoutMs: 20_000,
    },
    context.signal,
    async (environment) => {
      await environment.page.exposeFunction(
        "recordAssetRejection",
        (kind: string, value: unknown) =>
          environment.evidence.record(kind, value),
      );
      return environment.execute("asset rejection recovery", {}, () =>
        environment.page.evaluate(
          async ({ urls, scenarioPath }) => {
            const contract = await import(urls.generated);
            const { assetRejectionRecovery } = await import(
              `${urls.origin}/${scenarioPath}`
            );
            const host = await contract.IppHostClient.connectWorker(
              urls.workerScript,
              urls.wasm,
            );
            try {
              const record = (
                globalThis as unknown as {
                  recordAssetRejection(
                    kind: string,
                    value: unknown,
                  ): Promise<void>;
                }
              ).recordAssetRejection;
              return await assetRejectionRecovery(
                host,
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
              await host.close();
            }
          },
          { urls: environment.urls, scenarioPath },
        ),
      );
    },
  );
});
