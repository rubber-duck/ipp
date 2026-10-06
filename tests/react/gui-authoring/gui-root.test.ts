import assert from "node:assert/strict";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { runNativeEnvironment } from "../../harness/native.js";
import { runBrowserEnvironment } from "../../harness/browser.js";

test("ordinary React GUI through native WebSocket", {
  timeout: 90_000,
}, async (context) => {
  process.env.NODE_ENV = "production";
  const { guiAuthoring, reactLifecycleTransport, webSocketTransport } =
    await import("./pages/gui-authoring.js");
  const directory = resolve("target/integration-artifacts/native");
  const contract = await import(
    pathToFileURL(resolve("target/integration-artifacts/client/generated.js"))
      .href
  );
  await runNativeEnvironment(
    "react-gui-authoring-native",
    {
      executable: resolve(
        directory,
        process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
      ),
      schemaArtifact: resolve(directory, "contract.bin"),
      workingDirectory: process.cwd(),
      operationTimeoutMs: 60_000,
    },
    context.signal,
    async (environment) => {
      const observed = reactLifecycleTransport(
        contract,
        webSocketTransport(environment.url),
      );
      const host = await contract.IppHostClient.connectTransport(
        observed.transport,
      );
      try {
        const result = await environment.execute("ordinary React GUI", {}, () =>
          guiAuthoring(
            host,
            contract,
            () => contract.IppHostClient.connectWebSocket(environment.url),
            observed.probe,
          ),
        );
        assert.equal(result.length, 9);
      } finally {
        await host.close();
      }
    },
  );
});

for (const variant of ["development", "production"] as const) {
  test(`ordinary React GUI through worker WASM ${variant}`, {
    timeout: 90_000,
  }, async (context) => {
    const directory = resolve("target/browser-build/render");
    const build = {
      name: "render" as const,
      generatedModule: resolve(directory, "generated.js"),
      runtimeWasm: resolve(directory, "runtime.wasm"),
      contractArtifact: resolve(directory, "contract.bin"),
    };
    await runBrowserEnvironment(
      `react-gui-authoring-${variant}`,
      {
        workspace: process.cwd(),
        build,
        rendering: true,
        operationTimeoutMs: 60_000,
      },
      context.signal,
      async (environment) => {
        const result = await environment.execute("ordinary React GUI", {}, () =>
          environment.page.evaluate(
            async ({ urls, variant }) => {
              const contract = await import(urls.generated);
              const fixture = await import(
                `${urls.origin}/target/react-gui-authoring/fixture-${variant}.js`
              );
              const owner = fixture.createWorkerHost(
                urls.workerScript,
                urls.wasm,
                contract.MAX_MESSAGE_BYTES,
                { canvas: new OffscreenCanvas(1, 1) },
              );
              const connect = () =>
                contract.IppHostClient.connectTransport(owner.connect());
              const observed = fixture.reactLifecycleTransport(
                contract,
                owner.connect(),
              );
              const host = await contract.IppHostClient.connectTransport(
                observed.transport,
              );
              try {
                return (await fixture.guiAuthoring(
                  host,
                  contract,
                  connect,
                  observed.probe,
                )) as string[];
              } finally {
                await host.close();
                await owner.close();
              }
            },
            { urls: environment.urls, variant },
          ),
        );
        assert.equal(result.length, 9);
      },
    );
  });
}
