import assert from "node:assert/strict";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import type { WorldPersistenceHostClient } from "@ipp/client";
import { runNativeEnvironment } from "./environment.js";
import type { GuiTestClient } from "./scenarios/gui-lifecycle.js";
import { exerciseGuiReactRoot } from "./scenarios/gui-react-root.js";

test("React GuiRoot mounts through the producer lifecycle over native", {
  timeout: 60000,
}, async (context) => {
  const workspace = process.cwd();
  const profile = resolve(workspace, "target/gui-host");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  assert.equal(contract.CAPABILITIES.gui, true);
  await runNativeEnvironment(
    "gui-react-root",
    {
      executable: resolve(
        profile,
        process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
      ),
      schemaArtifact: resolve(profile, "contract.bin"),
      workingDirectory: workspace,
      operationTimeoutMs: 20000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/gui-react-root/native",
      ),
    },
    context.signal,
    async (env) => {
      const host = await env.track<WorldPersistenceHostClient<GuiTestClient>>(
        contract.IppHostClient.connectWebSocket(env.url, {
          signal: env.signal,
        }),
      );
      const result = await env.execute("gui react root", {}, () =>
        exerciseGuiReactRoot(host),
      );
      env.evidence.record("gui react root", result);
      assert.notEqual(result.incarnation, result.remountedIncarnation);
      assert.notEqual(result.bindingEntityEnrichment, "0");
      assert.equal(
        result.correctedSliderRevision,
        result.initialSliderRevision + 2,
      );
      assert.equal(result.mountBatchRequests, 1);
      assert.equal(result.mountBatchEdits, 50);
    },
  );
});
