import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { resolve } from "node:path";
import test from "node:test";
import { promisify } from "node:util";
import {
  Entity,
  IppHostClient,
  Scalar,
} from "../../target/integration-artifacts/client/generated.js";
import { runNativeEnvironment } from "../harness/native.js";

const execute = promisify(execFile);

test("headless CLI observes driven values and cleans up only its own World", {
  timeout: 30_000,
}, async (context) => {
  const workspace = process.cwd();
  await runNativeEnvironment(
    "headless-client",
    {
      executable: resolve(
        workspace,
        "target/integration-artifacts/native",
        process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
      ),
      schemaArtifact: resolve(
        workspace,
        "target/integration-artifacts/native.contract",
      ),
      workingDirectory: workspace,
      operationTimeoutMs: 10_000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/headless-client",
      ),
    },
    context.signal,
    async (environment) => {
      const host = await environment.track(
        IppHostClient.connectWebSocket(environment.url, {
          signal: environment.signal,
        }),
      );
      const peer = await host.createWorld({
        selectedSystems: ["ipp.constraints"],
        symbolicId: "headless-cli-peer",
      });
      try {
        const client = await host.openWorld(peer.reference);
        try {
          const outcome = await client.batch([
            Entity.create(1, { symbolicId: "sentinel", classes: [] }),
            Scalar.insert(Entity.alias(1), { value: 42 }),
          ]);
          assert.equal(outcome.ok, true);
          const expectedWorlds = await host.listWorlds();
          const peerEntities = (await client.inspect()).entities;
          for (let invocation = 0; invocation < 2; invocation++) {
            await environment.execute(
              "native-cli",
              { invocation },
              async () => {
                const result = await execute(
                  process.execPath,
                  [
                    resolve(workspace, "target/headless-client/main.js"),
                    environment.url,
                  ],
                  {
                    cwd: workspace,
                    signal: environment.signal,
                    timeout: 10_000,
                    maxBuffer: 1024 * 1024,
                  },
                );
                await environment.evidence.writeJson(
                  `cli-${invocation}.json`,
                  result,
                );
                assert.match(result.stdout, /After the initial frame:/);
                assert.match(result.stdout, /After updating the source:/);
                const observations = result.stdout
                  .split(/\r?\n/)
                  .filter((line) => line.startsWith("{"));
                assert.equal(observations.length, 2);
                assertScalars(JSON.parse(observations[0]!), 4, 9);
                assertScalars(JSON.parse(observations[1]!), 6, 13);
                assert.deepEqual(await host.listWorlds(), expectedWorlds);
                assert.deepEqual(
                  (await client.inspect()).entities,
                  peerEntities,
                );
                return { invocation, peer: peer.reference, expectedWorlds };
              },
            );
          }
        } finally {
          await client.close();
        }
      } finally {
        await host.destroyWorld(peer.reference);
      }
    },
  );
});

function assertScalars(value: unknown, source: number, driven: number): void {
  assert(value !== null && typeof value === "object" && "entities" in value);
  assert(Array.isArray(value.entities));
  assert.equal(value.entities.length, 2);
  // The driving constraint writes its evaluated value into the one stored
  // Scalar; it keeps the authored 99 as its original.
  const observed = value.entities.map((entity) => ({
    name: entity.metadata.symbolicId,
    value: entity.components.find(
      (component: { component: number }) => component.component === Scalar.id,
    )?.fields.value,
  }));
  observed.sort((left, right) => left.name.localeCompare(right.name));
  assert.deepEqual(observed, [
    { name: "driven", value: driven },
    { name: "source", value: source },
  ]);
}
