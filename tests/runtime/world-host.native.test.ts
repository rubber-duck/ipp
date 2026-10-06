import type { SpatialWorldClient } from "@ipp/client";
import type { HostedWorldClient } from "../fixtures/commands.js";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { runNativeEnvironment } from "../harness/native.js";
import {
  WORLD_HOST_SYSTEMS,
  worldHostCases,
} from "./scenarios/world-host-cases.js";
import { webSocketTransport } from "../../packages/ipp-client/src/transport.js";
import { contractClientsServed } from "../harness/assertions.js";
import { hostServesClientsWithAndWithoutItsContract } from "./scenarios/host-contract.js";

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
            environment.track<HostedWorldClient<SpatialWorldClient>>(
              contract.IppClient.connectWebSocket(environment.url, {
                selectedSystems: WORLD_HOST_SYSTEMS,
                signal: environment.signal,
              }),
            ),
        );
        return environment.execute(scenario.name, {}, () =>
          scenario.run(
            client,
            contract,
            environment.evidence.record.bind(environment.evidence),
            client.host,
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
                selectedSystems: WORLD_HOST_SYSTEMS,
                signal: environment.signal,
              }),
            ),
          contract,
          environment.evidence.record.bind(environment.evidence),
        ),
      ),
  );
});

test("native Host serves its contract and leaves compatibility to each client", {
  timeout: 30_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/world-host-build/native");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  // The client generated for the other host target, whose contract differs.
  const foreign = await import(
    pathToFileURL(
      resolve(workspace, "target/world-host-build/wasm/generated.js"),
    ).href
  );
  const built = await readFile(resolve(profile, "contract.bin"));
  await runNativeEnvironment(
    "native contract clients",
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
      const observation = await environment.execute(
        "clients with and without the Host's contract",
        { url: environment.url },
        () =>
          hostServesClientsWithAndWithoutItsContract(
            () => webSocketTransport(environment.url),
            built,
            contract,
            foreign,
            WORLD_HOST_SYSTEMS,
            10_000,
          ),
      );
      contractClientsServed(
        observation,
        contract.SCHEMA_HASH,
        foreign.SCHEMA_HASH,
      );
    },
  );
});
