import type { SpatialWorldClient } from "@ipp/client";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { runNativeEnvironment } from "./environment.js";
import { worldHostCases } from "./world-host-cases.js";

for (const scenario of worldHostCases) {
  test(`native ${scenario.name}`, { timeout: 30_000 }, async (context) => {
    const workspace = resolve(process.cwd());
    const profile = resolve(workspace, "target/world-host-build/native");
    const contract = await import(
      pathToFileURL(resolve(profile, "generated.js")).href
    );
    await runNativeEnvironment(
      `native ${scenario.name}`,
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
        const client = await environment.execute(
          "client connect",
          { url: environment.url },
          () =>
            environment.track<SpatialWorldClient>(
              contract.IppClient.connectWebSocket(environment.url, {
                signal: environment.signal,
              }),
            ),
        );
        return environment.execute(scenario.name, {}, () =>
          scenario.run(
            client,
            contract,
            environment.evidence.record.bind(environment.evidence),
          ),
        );
      },
    );
  });
}

test("native worlds share Host assets and isolate producer/session state", {
  timeout: 30_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/world-host-build/native");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  const { multipleWorldsShareAssets } = await import(
    "./scenarios/multiple-worlds.js"
  );
  await runNativeEnvironment(
    "native shared Host worlds",
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
      environment.execute("multiple worlds", {}, () =>
        multipleWorldsShareAssets(
          () =>
            environment.track(
              contract.IppClient.connectWebSocket(environment.url, {
                signal: environment.signal,
              }),
            ),
          contract,
          environment.evidence.record.bind(environment.evidence),
        ),
      ),
  );
});
