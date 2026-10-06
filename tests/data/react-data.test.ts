import { resolve } from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import { runNativeEnvironment } from "../harness/native.js";
import { runBrowserEnvironment } from "../harness/browser.js";
import { reactData } from "./scenarios/react-data.js";
import type { DatasetHost } from "./scenarios/datasets.js";

const workspace = process.cwd();
test("native WebSocket React data declarations and producer lifecycle", {
  timeout: 90_000,
}, async (context) => {
  const contract = await import(
    pathToFileURL(resolve("target/integration-artifacts/client/generated.js"))
      .href
  );
  const configuration = {
    executable: resolve(
      "target/integration-artifacts/native",
      process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
    ),
    schemaArtifact: resolve("target/integration-artifacts/native.contract"),
    workingDirectory: workspace,
    operationTimeoutMs: 30_000,
  };
  await runNativeEnvironment(
    "react-data-native",
    configuration,
    context.signal,
    async (environment) => {
      const host = await environment.track<DatasetHost>(
        contract.IppHostClient.connectWebSocket(environment.url, {
          signal: environment.signal,
        }),
      );
      return environment.execute(
        "react-data",
        { fixture: "react-binding-source-v1" },
        () =>
          reactData(host, contract, (label, value) =>
            environment.evidence.record(label, value),
          ),
      );
    },
  );
});

for (const mode of ["development", "production"])
  test(`worker WASM React data declarations and producer lifecycle ${mode}`, {
    timeout: 90_000,
  }, async (context) => {
    const directory = resolve("target/browser-build/headless");
    await runBrowserEnvironment(
      `react-data-worker-${mode}`,
      {
        workspace,
        operationTimeoutMs: 60_000,
        build: {
          name: "headless",
          generatedModule: resolve(directory, "generated.js"),
          runtimeWasm: resolve(directory, "runtime.wasm"),
          contractArtifact: resolve(directory, "contract.bin"),
        },
      },
      context.signal,
      async (environment) => {
        await environment.page.exposeFunction(
          "recordData",
          (label: string, value: unknown) =>
            environment.evidence.record(label, value),
        );
        return environment.execute(
          "react-data",
          { fixture: "react-binding-source-v1", mode },
          () =>
            environment.page.evaluate(
              async (urls) => {
                const driver = await import(urls.driver);
                return driver.workerReactData(urls);
              },
              {
                generated: environment.urls.generated,
                wasm: environment.urls.wasm,
                worker: `${environment.urls.origin}/target/datasets/dataset-worker-${mode}.js`,
                driver: `${environment.urls.origin}/target/datasets/react-data-driver.js`,
              },
            ),
        );
      },
    );
  });
