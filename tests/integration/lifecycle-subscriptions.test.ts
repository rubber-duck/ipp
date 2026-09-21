import type { SpatialWorldClient } from "@ipp/client";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { runNativeEnvironment } from "./environment.js";
import { lifecycleHostCases } from "./lifecycle-subscriptions-cases.js";

for (const scenario of lifecycleHostCases) {
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

test("native lifecycle subscriptions end at detach and client close", {
  timeout: 30_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/world-host-build/native");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  const { lifecycleSessionIsolation } = await import(
    "./scenarios/lifecycle-subscriptions.js"
  );
  await runNativeEnvironment(
    "native lifecycle session isolation",
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
        Parameters<typeof lifecycleSessionIsolation>[0]
      >(
        contract.IppHostClient.connectWebSocket(environment.url, {
          signal: environment.signal,
        }),
      );
      return environment.execute("lifecycle detach and close", {}, () =>
        lifecycleSessionIsolation(host),
      );
    },
  );
});

test("native lifecycle subscriptions isolate shared sessions and independent Worlds", {
  timeout: 30_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/world-host-build/native");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  const { sharedLifecycleSubscriptions } = await import(
    "./scenarios/lifecycle-subscriptions.js"
  );
  await runNativeEnvironment(
    "native lifecycle shared sessions",
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
      environment.execute("lifecycle shared sessions", {}, () =>
        sharedLifecycleSubscriptions(
          () =>
            contract.IppHostClient.connectWebSocket(environment.url, {
              signal: environment.signal,
            }),
          environment.evidence.record.bind(environment.evidence),
        ),
      ),
  );
});
