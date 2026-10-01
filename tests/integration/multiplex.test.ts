import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { runNativeEnvironment } from "./environment.js";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
} from "../browser/environment.js";
import {
  concurrentAttachedWorlds,
  multiplexSessions,
} from "./scenarios/multiplex.js";
import {
  hostLifecycleParticipant,
  hostLifecycle,
  graphTransfers,
  attachmentReceipts,
  streamedWorldCommands,
} from "./host-transport-driver.js";
import { webSocketTransport } from "../../packages/ipp-client/src/transport.js";

test("native multiplex sessions, exact references and open-batch fairness", {
  timeout: 60_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/world-host-build/native");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  await runNativeEnvironment(
    "multiplex-native",
    {
      executable: resolve(
        profile,
        process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
      ),
      schemaArtifact: resolve(profile, "contract.bin"),
      workingDirectory: workspace,
      operationTimeoutMs: 20_000,
    },
    context.signal,
    async (environment) =>
      environment.execute("multiplex", {}, async () => {
        const host = await environment.track<
          Parameters<typeof multiplexSessions>[0]
        >(
          contract.IppHostClient.connectWebSocket(environment.url, {
            signal: environment.signal,
          }),
        );
        const multiplex = await multiplexSessions(host);
        const credit = await concurrentAttachedWorlds(host);
        const receipts = await attachmentReceipts(host);
        const lifecycle = await hostLifecycle(() =>
          hostLifecycleParticipant(
            contract,
            webSocketTransport(environment.url),
          ),
        );
        const graph = await graphTransfers(() =>
          hostLifecycleParticipant(
            contract,
            webSocketTransport(environment.url),
          ),
        );
        const streaming = await streamedWorldCommands(host, contract);
        return { multiplex, credit, lifecycle, graph, receipts, streaming };
      }),
  );
});

test("browser worker multiplex sessions, exact references and open-batch fairness", {
  timeout: 60_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/world-host-build/wasm");
  const build: BrowserBuildConfiguration = {
    name: "world-host",
    generatedModule: resolve(profile, "generated.js"),
    runtimeWasm: resolve(profile, "runtime.wasm"),
    contractArtifact: resolve(profile, "contract.bin"),
  };
  await runBrowserEnvironment(
    "multiplex-worker",
    { workspace, build, operationTimeoutMs: 20_000 },
    context.signal,
    async (environment) =>
      environment.execute("multiplex", {}, () =>
        environment.page.evaluate(async (urls) => {
          const contract = await import(urls.generated);
          const scenarios = await import(
            `${urls.origin}/target/multiplex-tests/scenario.js`
          );
          const host = await contract.IppHostClient.connectWorker(
            urls.workerScript,
            urls.wasm,
          );
          try {
            const multiplex = await scenarios.multiplexSessions(host);
            const credit = await scenarios.concurrentAttachedWorlds(host);
            const lifecycleDriver = await import(
              `${urls.origin}/target/multiplex-tests/host-lifecycle.js`
            );
            const lifecycle = await lifecycleDriver.hostLifecycle(() =>
              lifecycleDriver.hostLifecycleParticipant(
                contract,
                lifecycleDriver.workerTransport(
                  urls.workerScript,
                  urls.wasm,
                  contract.MAX_MESSAGE_BYTES,
                ),
              ),
            );
            const graph = await lifecycleDriver.graphTransfers(() =>
              lifecycleDriver.hostLifecycleParticipant(
                contract,
                lifecycleDriver.workerTransport(
                  urls.workerScript,
                  urls.wasm,
                  contract.MAX_MESSAGE_BYTES,
                ),
              ),
            );
            const receipts = await lifecycleDriver.attachmentReceipts(host);
            const streaming = await lifecycleDriver.streamedWorldCommands(
              host,
              contract,
            );
            return {
              multiplex,
              credit,
              lifecycle,
              graph,
              receipts,
              streaming,
            };
          } finally {
            await host.close();
          }
        }, environment.urls),
      ),
  );
});
