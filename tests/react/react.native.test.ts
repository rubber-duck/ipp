import assert from "node:assert/strict";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import type { Client } from "@ipp/client";
import { runNativeEnvironment } from "../harness/native.js";
import { REACT_ROOT, selectSystems } from "../fixtures/system-selections.js";

test("React entity links reconcile through native WebSocket", {
  timeout: 60000,
}, async (context) => {
  process.env.NODE_ENV = "production";
  const { exerciseEntityLinks } = await import("./scenarios/hierarchy.js");
  const workspace = process.cwd();
  const profile = resolve(workspace, "target/world-host-build/native");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  await runNativeEnvironment(
    "react entity links",
    {
      executable: resolve(
        profile,
        process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
      ),
      schemaArtifact: resolve(profile, "contract.bin"),
      workingDirectory: workspace,
      operationTimeoutMs: 20000,
    },
    context.signal,
    async (environment) => {
      const client = await environment.track<Client>(
        contract.IppClient.connectWebSocket(environment.url, {
          selectedSystems: selectSystems(REACT_ROOT),
          signal: environment.signal,
        }),
      );
      const report = await environment.execute(
        "React link reconciliation",
        {},
        () => exerciseEntityLinks(client, contract),
      );
      environment.evidence.record("React link reconciliation", report);
      assert.equal(report.length, 26);
    },
  );
});
