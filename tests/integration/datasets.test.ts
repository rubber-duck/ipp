import { resolve } from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import { runNativeEnvironment } from "./environment.js";
import { runBrowserEnvironment } from "../browser/environment.js";
import { datasets, type DatasetHost } from "./scenarios/datasets.js";

const workspace = process.cwd();
const native = resolve("target/integration-artifacts/native");
const headless = resolve("target/browser-build/headless");

test("native WebSocket datasets through matching generated client", {
  timeout: 60_000,
}, async (context) => {
  const contract = await import(
    pathToFileURL(resolve("target/integration-artifacts/client/generated.js"))
      .href
  );
  await runNativeEnvironment(
    "datasets-native",
    {
      executable: resolve(
        native,
        process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
      ),
      schemaArtifact: resolve("target/integration-artifacts/native.contract"),
      workingDirectory: workspace,
      operationTimeoutMs: 20_000,
    },
    context.signal,
    async (environment) => {
      const connect = () =>
        environment.track<DatasetHost>(
          contract.IppHostClient.connectWebSocket(environment.url, {
            signal: environment.signal,
          }),
        );
      const host = await connect();
      return environment.execute(
        "datasets",
        { fixture: "typed-source-v1" },
        () => datasets(host, connect),
      );
    },
  );
});

for (const mode of ["development", "production"])
  test(`worker WASM datasets through executed-target client ${mode}`, {
    timeout: 60_000,
  }, async (context) => {
    await runBrowserEnvironment(
      `datasets-worker-${mode}`,
      {
        workspace,
        operationTimeoutMs: 20_000,
        build: {
          name: "headless",
          generatedModule: resolve(headless, "generated.js"),
          runtimeWasm: resolve(headless, "runtime.wasm"),
          contractArtifact: resolve(headless, "contract.bin"),
        },
      },
      context.signal,
      async (environment) =>
        environment.execute(
          "datasets",
          { fixture: "typed-source-v1", mode },
          () =>
            environment.page.evaluate(
              async (urls) => {
                const driver = await import(urls.driver);
                return driver.workerDatasets(urls);
              },
              {
                generated: environment.urls.generated,
                wasm: environment.urls.wasm,
                worker: `${environment.urls.origin}/target/datasets/dataset-worker-${mode}.js`,
                driver: `${environment.urls.origin}/target/datasets/dataset-driver.js`,
              },
            ),
        ),
    );
  });
