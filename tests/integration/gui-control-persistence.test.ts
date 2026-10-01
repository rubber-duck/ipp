import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { runNativeEnvironment } from "./environment.js";
import {
  type BrowserBuildConfiguration,
  runBrowserEnvironment,
} from "../browser/environment.js";
import { exerciseOrdinaryGuiPersistence } from "./scenarios/gui-control-persistence.js";

test("ordinary GUI persistence through native WebSocket", {
  timeout: 90_000,
}, async (context) => {
  const profile = resolve("target/integration-artifacts/native");
  const contract = await import(
    pathToFileURL(resolve("target/integration-artifacts/client/generated.js"))
      .href
  );
  await runNativeEnvironment(
    "gui-control-persistence-native",
    {
      executable: resolve(
        profile,
        process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
      ),
      schemaArtifact: resolve(profile, "contract.bin"),
      workingDirectory: process.cwd(),
      operationTimeoutMs: 60_000,
    },
    context.signal,
    async (environment) =>
      environment.execute(
        "ordinary-gui-persistence",
        { rendering: false },
        async () => {
          const host = await contract.IppHostClient.connectWebSocket(
            environment.url,
            { signal: environment.signal },
          );
          try {
            return await exerciseOrdinaryGuiPersistence(host);
          } finally {
            await host.close();
          }
        },
      ),
  );
});

test("ordinary GUI persistence through worker WASM", {
  timeout: 90_000,
}, async (context) => {
  const profile = resolve("target/browser-build/headless");
  const build: BrowserBuildConfiguration = {
    name: "headless",
    generatedModule: resolve(profile, "generated.js"),
    runtimeWasm: resolve(profile, "runtime.wasm"),
    contractArtifact: resolve(profile, "contract.bin"),
  };
  await runBrowserEnvironment(
    "gui-control-persistence-worker",
    {
      workspace: process.cwd(),
      build,
      operationTimeoutMs: 60_000,
    },
    context.signal,
    async (environment) =>
      environment.execute(
        "ordinary-gui-persistence",
        { rendering: false },
        () =>
          environment.page.evaluate(async (urls) => {
            const contract = await import(urls.generated);
            const scenario = await import(
              `${urls.origin}/target/multiplex-tests/gui-control-persistence.js`
            );
            const host = await contract.IppHostClient.connectWorker(
              urls.workerScript,
              urls.wasm,
            );
            try {
              return await scenario.exerciseOrdinaryGuiPersistence(host);
            } finally {
              await host.close();
            }
          }, environment.urls),
      ),
  );
});
