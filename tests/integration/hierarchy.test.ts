import type { AnimationWorldClient } from "@ipp/client";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { runNativeEnvironment } from "./environment.js";
import { hierarchyLifecycle } from "./scenarios/hierarchy.js";

test("native hierarchy and look-at lifecycle through the generated WebSocket client", {
  timeout: 30_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/world-host-build/native");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  await runNativeEnvironment(
    "hierarchy lifecycle",
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
    async (environment) => {
      const client = await environment.track<AnimationWorldClient>(
        contract.IppClient.connectWebSocket(environment.url, {
          signal: environment.signal,
        }),
      );
      await environment.execute("hierarchy lifecycle", {}, () =>
        hierarchyLifecycle(
          client,
          contract,
          environment.evidence.record.bind(environment.evidence),
        ),
      );
    },
  );
});
