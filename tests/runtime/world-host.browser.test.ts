import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";
import { contractClientsServed } from "../harness/assertions.js";
import { worldHostCases } from "./scenarios/world-host-cases.js";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
} from "../harness/browser.js";

for (const scenario of worldHostCases) {
  test(`browser ${scenario.name}`, { timeout: 30_000 }, async (context) => {
    const workspace = resolve(process.cwd());
    const profile = resolve(workspace, "target/world-host-build/wasm");
    const build: BrowserBuildConfiguration = {
      name: "world-host",
      generatedModule: resolve(profile, "generated.js"),
      runtimeWasm: resolve(profile, "runtime.wasm"),
      contractArtifact: resolve(profile, "contract.bin"),
    };
    await runBrowserEnvironment(
      `browser ${scenario.name}`,
      {
        workspace,
        build,
        operationTimeoutMs: 20_000,
      },
      context.signal,
      async (environment) => {
        await environment.page.exposeFunction(
          "recordWorldHost",
          (kind: string, value: unknown) =>
            environment.evidence.record(kind, value),
        );
        return environment.execute(scenario.name, {}, () =>
          environment.page.evaluate(
            async ({ urls, name }) => {
              const contract = await import(urls.generated);
              const {
                worldHostCases: cases,
                WORLD_HOST_SYSTEMS: selectedSystems,
              } = await import(
                `${urls.origin}/dist/tests/runtime/scenarios/world-host-cases.js`
              );
              const scenario = cases.find(
                (candidate: { name: string }) => candidate.name === name,
              );
              if (!scenario)
                throw new Error(`Missing scene host scenario: ${name}`);
              const client = await contract.IppClient.connectWorker(
                urls.workerScript,
                urls.wasm,
                { selectedSystems },
              );
              try {
                const record = (
                  globalThis as unknown as {
                    recordWorldHost(
                      kind: string,
                      value: unknown,
                    ): Promise<void>;
                  }
                ).recordWorldHost;
                return await scenario.run(
                  client,
                  contract,
                  (kind: string, value: unknown) =>
                    record(
                      kind,
                      JSON.parse(
                        JSON.stringify(value, (_key, item: unknown) =>
                          typeof item === "bigint"
                            ? { $bigint: item.toString() }
                            : item,
                        ),
                      ),
                    ),
                  client.host,
                );
              } finally {
                await client.close();
              }
            },
            { urls: environment.urls, name: scenario.name },
          ),
        );
      },
    );
  });
}

test("browser worker Host serves its contract and leaves compatibility to each client", {
  timeout: 30_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/world-host-build/wasm");
  const build: BrowserBuildConfiguration = {
    name: "world-host",
    generatedModule: resolve(profile, "generated.js"),
    runtimeWasm: resolve(profile, "runtime.wasm"),
    contractArtifact: resolve(profile, "contract.bin"),
  };
  const built = Array.from(await readFile(build.contractArtifact));
  await runBrowserEnvironment(
    "browser contract clients",
    { workspace, build, operationTimeoutMs: 20_000 },
    context.signal,
    async (environment) => {
      const observation = await environment.execute(
        "clients with and without the Host's contract",
        { contract: environment.urls.mismatchGenerated },
        () =>
          environment.page.evaluate(
            async ({ urls, built }) => {
              const packageUrl = `${urls.origin}/dist/packages/ipp-client/src`;
              const { createWorkerHost } = await import(
                `${packageUrl}/worker.js`
              );
              const { HOST_MESSAGE_BYTES } = await import(
                `${packageUrl}/host-protocol.js`
              );
              const { hostServesClientsWithAndWithoutItsContract } =
                await import(
                  `${urls.origin}/dist/tests/runtime/scenarios/host-contract.js`
                );
              const { WORLD_HOST_SYSTEMS } = await import(
                `${urls.origin}/dist/tests/runtime/scenarios/world-host-cases.js`
              );
              // One worker Host; each client opens its own connection to it.
              const host = createWorkerHost(
                urls.workerScript,
                urls.wasm,
                HOST_MESSAGE_BYTES,
              );
              try {
                const observation =
                  await hostServesClientsWithAndWithoutItsContract(
                    () => host.connect(),
                    built,
                    await import(urls.generated),
                    await import(urls.mismatchGenerated),
                    WORLD_HOST_SYSTEMS,
                    10_000,
                  );
                return {
                  observation,
                  hashes: {
                    host: (await import(urls.generated)).SCHEMA_HASH.toString(),
                    foreign: (
                      await import(urls.mismatchGenerated)
                    ).SCHEMA_HASH.toString(),
                  },
                };
              } finally {
                await host.close();
              }
            },
            { urls: environment.urls, built },
          ),
      );
      contractClientsServed(
        observation.observation,
        BigInt(observation.hashes.host),
        BigInt(observation.hashes.foreign),
      );
    },
  );
});
