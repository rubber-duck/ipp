import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { runNativeEnvironment } from "../harness/native.js";
import { runBrowserEnvironment } from "../harness/browser.js";
import { blenderHeadless } from "./scenarios/blender-headless.js";
import {
  blenderCleanup,
  blenderReplyGate,
} from "./drivers/browser-blender-headless.js";
import { webSocketTransport } from "../../packages/ipp-client/src/transport.js";

test("Blender links, graph files and held-ACK cleanup over native WebSocket", {
  timeout: 90_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/world-host-build/native");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  await runNativeEnvironment(
    "blender-headless-native",
    {
      executable: resolve(profile, "ipp-server"),
      schemaArtifact: resolve(profile, "contract.bin"),
      workingDirectory: workspace,
      operationTimeoutMs: 25_000,
      evidenceParent: resolve("target/integration-artifacts/blender-headless"),
    },
    context.signal,
    (environment) =>
      environment.execute("blender-headless-native", {}, async () => {
        const gate = blenderReplyGate(
          contract,
          webSocketTransport(environment.url),
        );
        const host = await environment.track<
          Parameters<typeof blenderHeadless>[0]
        >(
          contract.IppHostClient.connectTransport(gate.transport, {
            signal: environment.signal,
          }),
        );
        const hierarchy = await blenderHeadless(host, contract, false);
        const cleanup = [];
        for (const kind of ["controller", "asset"] as const)
          cleanup.push(await blenderCleanup(host, contract, gate, kind));
        return { hierarchy, cleanup };
      }),
  );
});

test("Blender links, ParentJoint and held-ACK cleanup over worker WASM", {
  timeout: 90_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/browser-build/render");
  const build = {
    name: "render" as const,
    generatedModule: resolve(profile, "generated.js"),
    runtimeWasm: resolve(profile, "runtime.wasm"),
    contractArtifact: resolve(profile, "contract.bin"),
  };
  await runBrowserEnvironment(
    "blender-headless-worker",
    {
      workspace,
      build,
      rendering: false,
      operationTimeoutMs: 25_000,
      evidenceParent: resolve("target/integration-artifacts/blender-headless"),
    },
    context.signal,
    (environment) =>
      environment.execute("blender-headless-worker", {}, () =>
        environment.page.evaluate(async (urls) => {
          const contract = await import(urls.generated);
          const scenario = await import(
            `${urls.origin}/target/blender-headless/scenario.js`
          );
          const canvas = document.createElement("canvas");
          canvas.width = 320;
          canvas.height = 240;
          const gate = scenario.blenderReplyGate(
            contract,
            scenario.workerTransport(
              urls.workerScript,
              urls.wasm,
              contract.MAX_MESSAGE_BYTES,
              { canvas: canvas.transferControlToOffscreen() },
            ),
          );
          const host = await contract.IppHostClient.connectTransport(
            gate.transport,
          );
          try {
            const hierarchy = await scenario.blenderHeadless(
              host,
              contract,
              true,
            );
            const cleanup = [];
            for (const kind of ["controller", "asset"])
              cleanup.push(
                await scenario.blenderCleanup(host, contract, gate, kind),
              );
            return { hierarchy, cleanup };
          } finally {
            await host.close();
          }
        }, environment.urls),
      ),
  );
});
