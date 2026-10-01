import assert from "node:assert/strict";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { runNativeEnvironment } from "./environment.js";
import type { GuiContract, GuiHost } from "./scenarios/gui-lifecycle.js";
import { exerciseGuiReactRoot } from "./scenarios/gui-react-root.js";

interface GeneratedHost extends GuiContract {
  IppHostClient: {
    connectWebSocket(
      url: string,
      options: { signal: AbortSignal },
    ): Promise<GuiHost>;
  };
}

test("React ordinary GUI declarations mount, update and unmount over native", {
  timeout: 60_000,
}, async (context) => {
  const workspace = process.cwd();
  const profile = resolve(workspace, "target/integration-artifacts/native");
  const contract = (await import(
    pathToFileURL(
      resolve(workspace, "target/integration-artifacts/client/generated.js"),
    ).href
  )) as GeneratedHost;
  await runNativeEnvironment(
    "gui-react-root",
    {
      executable: resolve(
        profile,
        process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
      ),
      schemaArtifact: resolve(profile, "contract.bin"),
      workingDirectory: workspace,
      operationTimeoutMs: 30_000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/gui-react-root/native",
      ),
    },
    context.signal,
    async (env) => {
      const host = await env.track(
        contract.IppHostClient.connectWebSocket(env.url, {
          signal: env.signal,
        }),
      );
      const result = await env.execute("gui react root", {}, () =>
        exerciseGuiReactRoot(host, contract),
      );
      env.evidence.record("gui react root", result);
      assert.notEqual(result.panel, result.remountedPanel);
      assert.notEqual(result.symbolResolution, "0");
      assert.equal(result.correctedSliderValue.kind, "scalar");
      assert.ok(Math.abs(result.correctedSliderValue.value - 0.95) < 1e-6);
      assert.equal(result.mountBatchRequests, 1);
      assert.equal(result.mountDeclarations, 50);
      assert.equal(result.themeEntities.length, 2);
      assert.equal(result.themeEditsAfterSettle, 0);
      const order = [
        "toggle",
        "capture:root",
        "bubble:checkbox",
        "bubble:row",
        "bubble:root",
      ];
      // The current value, then the toggled value.
      assert.deepEqual(result.rootActionOrder, [order, order]);
    },
  );
});
