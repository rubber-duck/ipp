import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { runNativeEnvironment } from "../harness/native.js";
import {
  type BrowserBuildConfiguration,
  runBrowserEnvironment,
} from "../harness/browser.js";
import { ordinaryGui } from "./scenarios/gui-local.js";
import {
  ordinaryGuiObservations,
  foreignGuiObservers,
  uncertainGuiControlErrors,
} from "./scenarios/gui-observations.js";
import {
  guiLocalTransport,
  webSocketTransport,
} from "./drivers/browser-gui-local.js";

test("ordinary GUI native WebSocket terminal delivery", {
  timeout: 90_000,
}, async (context) => {
  const profile = resolve("target/integration-artifacts/native");
  const contract = await import(
    pathToFileURL(resolve("target/integration-artifacts/client/generated.js"))
      .href
  );
  await runNativeEnvironment(
    "gui-local-native",
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
      environment.execute("ordinary-gui", {}, async () => {
        const controlled = guiLocalTransport(
          contract,
          webSocketTransport(environment.url),
        );
        const host = await contract.IppHostClient.connectTransport(
          controlled.transport,
          { signal: environment.signal },
        );
        try {
          const semantics = await ordinaryGui(host, controlled.probe);
          const observations = await ordinaryGuiObservations(
            host,
            controlled.probe,
          );
          const controlErrors = await uncertainGuiControlErrors(
            host,
            controlled.probe,
          );
          const congested = await contract.IppHostClient.connectTransport(
            webSocketTransport(environment.url),
            { signal: environment.signal },
          );
          const healthy = await contract.IppHostClient.connectTransport(
            webSocketTransport(environment.url),
            { signal: environment.signal },
          );
          try {
            const isolation = await foreignGuiObservers(
              host,
              congested,
              healthy,
            );
            return { semantics, observations, isolation, controlErrors };
          } finally {
            await Promise.allSettled([congested.close(), healthy.close()]);
          }
        } finally {
          await host.close();
        }
      }),
  );
});

test("ordinary GUI worker WASM terminal delivery", {
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
    "gui-local-worker",
    {
      workspace: process.cwd(),
      build,
      operationTimeoutMs: 60_000,
    },
    context.signal,
    async (environment) =>
      environment.execute("ordinary-gui", {}, () =>
        environment.page.evaluate(async (urls) => {
          const contract = await import(urls.generated);
          const scenario = await import(
            `${urls.origin}/target/multiplex-tests/gui-local.js`
          );
          const observations = await import(
            `${urls.origin}/target/multiplex-tests/gui-observations.js`
          );
          const harness = await import(
            `${urls.origin}/target/multiplex-tests/gui-local-transport.js`
          );
          const controlled = harness.guiLocalTransport(
            contract,
            harness.workerTransport(
              urls.workerScript,
              urls.wasm,
              contract.MAX_MESSAGE_BYTES,
            ),
          );
          const host = await contract.IppHostClient.connectTransport(
            controlled.transport,
          );
          try {
            const semantics = await scenario.ordinaryGui(
              host,
              controlled.probe,
            );
            const observed = await observations.ordinaryGuiObservations(
              host,
              controlled.probe,
            );
            const controlErrors = await observations.uncertainGuiControlErrors(
              host,
              controlled.probe,
            );
            const delivery = await import(
              `${urls.origin}/target/multiplex-tests/worker-observer-delivery.js`
            );
            const endpoint = await delivery.workerObserverDelivery(urls);
            const terminatedEndpoint = await delivery.workerObserverDelivery(
              urls,
              true,
            );
            const productionDelivery = await import(
              `${urls.origin}/target/multiplex-tests/worker-observer-delivery.production.js`
            );
            const productionTerminatedEndpoint =
              await productionDelivery.workerObserverDelivery(
                urls,
                true,
                "production",
              );
            return {
              semantics,
              observations: observed,
              endpoint,
              terminatedEndpoint,
              productionTerminatedEndpoint,
              controlErrors,
            };
          } finally {
            await host.close();
          }
        }, environment.urls),
      ),
  );
});
