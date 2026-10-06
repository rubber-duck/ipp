import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { runNativeEnvironment } from "../harness/native.js";
import { assetRejectionRecovery } from "./scenarios/asset-rejection.js";

test("native asset rejection recovery", {
  timeout: 30_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/world-host-build/native");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  await runNativeEnvironment(
    "native asset rejection recovery",
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
      const host = await environment.track<
        Parameters<typeof assetRejectionRecovery>[0]
      >(
        contract.IppHostClient.connectWebSocket(environment.url, {
          signal: environment.signal,
        }),
      );
      return environment.execute("asset rejection recovery", {}, () =>
        assetRejectionRecovery(
          host,
          environment.evidence.record.bind(environment.evidence),
        ),
      );
    },
  );
});
