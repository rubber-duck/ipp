import type { Client, WorldPersistenceHostClient } from "@ipp/client";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { once } from "node:events";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { runNativeEnvironment } from "../harness/native.js";
import {
  createEntity,
  insertComponent,
  successfulBatch,
} from "../fixtures/commands.js";
import { RENDER, selectSystems } from "../fixtures/system-selections.js";

// A valid CPU mesh spanning multiple acquisition quanta. The HTTP fixture owns
// its immutable bytes; the same generated-client observations apply to a remote
// producer without relying on transport chunk boundaries.
function meshBytes(): Buffer {
  const vertices = 4096;
  const bytes = Buffer.alloc(16 + vertices * 24 + 6);
  bytes.write("IPPM");
  bytes.writeUInt32LE(1, 4);
  bytes.writeUInt32LE(vertices, 8);
  bytes.writeUInt32LE(3, 12);
  for (let vertex = 0; vertex < vertices; vertex++) {
    const offset = 16 + vertex * 24;
    bytes.writeFloatLE(vertex % 64, offset);
    bytes.writeFloatLE(Math.floor(vertex / 64), offset + 4);
    for (let channel = 0; channel < 3; channel++)
      bytes.writeFloatLE(0.5, offset + 12 + 4 * channel);
  }
  for (let index = 0; index < 3; index++)
    bytes.writeUInt16LE([0, 1, 64][index]!, bytes.length - 6 + index * 2);
  return bytes;
}

test("configured native HTTP source waits independently of World frames and rejects pending descriptors", {
  timeout: 30_000,
}, async (context) => {
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  let requested!: () => void;
  const seen = new Promise<void>((resolve) => {
    requested = resolve;
  });
  const bytes = meshBytes();
  let requests = 0;
  const producer = createServer(async (request, response) => {
    if (request.url === "/pending") {
      response.writeHead(202, { "content-type": "application/json" });
      response.end('{"availability":"pending"}');
      return;
    }
    if (request.url !== "/mesh") {
      response.writeHead(404);
      response.end();
      return;
    }
    requests++;
    response.writeHead(200, {
      "content-length": bytes.length,
      etag: '"immutable-mesh"',
    });
    response.flushHeaders();
    requested();
    await gate;
    response.end(bytes);
  });
  producer.listen(0, "127.0.0.1");
  await once(producer, "listening");
  const address = producer.address();
  assert.ok(address && typeof address !== "string");
  const prefix = `http://127.0.0.1:${address.port}/`;
  const workspace = process.cwd();
  const contract = await import(
    pathToFileURL(
      resolve(workspace, "target/integration-artifacts/client/generated.js"),
    ).href
  );
  try {
    await runNativeEnvironment(
      "native-http-io",
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
        extraArguments: ["--http-prefix", prefix],
        operationTimeoutMs: 12_000,
      },
      context.signal,
      async (environment) =>
        environment.execute(
          "gated native HTTP decode",
          { prefix, length: bytes.length },
          async () => {
            const host = await environment.track<
              WorldPersistenceHostClient<Client>
            >(
              contract.IppHostClient.connectWebSocket(environment.url, {
                signal: environment.signal,
              }),
            );
            const world = await host.createWorld({
              selectedSystems: selectSystems(RENDER),
              temporary: true,
            });
            const client = await host.openWorld(world.reference);
            const sources = [
              `${prefix}mesh`,
              `${prefix}pending`,
              "ipp://mesh/cube?width=1&height=1&length=1",
            ];
            successfulBatch(
              await client.batch(
                sources.flatMap((source, alias) => [
                  createEntity(alias, `mesh-${alias}`),
                  insertComponent(
                    client,
                    "MeshInstance",
                    { kind: "alias", alias },
                    { source },
                  ),
                ]),
              ),
            );
            await Promise.race([
              seen,
              new Promise((_, reject) =>
                setTimeout(
                  () => reject(new Error("HTTP acquisition never started")),
                  10_000,
                ).unref(),
              ),
            ]);
            const initial = await client.inspect();
            let tick = initial.tick;
            for (let frame = 0; frame < 3; frame++)
              tick = (await client.waitForFrame(tick)).tick;
            assert.ok(tick > initial.tick, "pending IO held World time");
            const pending = await client.inspect();
            assert.ok(
              pending.resources.some(
                (resource: { source: string; status: string }) =>
                  resource.source === sources[0] &&
                  resource.status !== "loaded" &&
                  resource.status !== "failed",
              ),
            );
            assert.ok(
              pending.resources.some(
                (resource: { source: string; status: string }) =>
                  resource.source === sources[2] &&
                  resource.status === "loaded",
              ),
              "independent builtin decode was starved",
            );
            release();
            const deadline = Date.now() + 10_000;
            let state = pending;
            while (
              !state.resources.some(
                (resource: { source: string; status: string }) =>
                  resource.source === sources[0] &&
                  resource.status === "loaded",
              )
            ) {
              assert.ok(Date.now() < deadline, "HTTP source did not complete");
              assert.ok(
                !state.resources.some(
                  (resource) =>
                    resource.source === sources[0] &&
                    resource.status === "failed",
                ),
                `HTTP decode failed: ${JSON.stringify(state.resources, (_, value) => (typeof value === "bigint" ? String(value) : value))}`,
              );
              await client.waitForFrame(state.tick);
              state = await client.inspect();
            }
            const loaded = state.resources.filter(
              (resource: { source: string }) => resource.source === sources[0],
            );
            assert.ok(
              loaded.every(
                (resource: { representation: { sourceBytes: bigint } }) =>
                  resource.representation.sourceBytes === BigInt(bytes.length),
              ),
            );
            assert.ok(
              state.resources.some(
                (resource: {
                  source: string;
                  status: string;
                  error?: string;
                }) =>
                  resource.source === sources[1] &&
                  resource.status === "failed" &&
                  resource.error?.includes("complete representation"),
              ),
              "HTTP202 descriptor was treated as asset input",
            );
            return {
              bytes: bytes.length,
              requests,
              worldFramesWhilePending: String(tick - initial.tick),
              resources: state.resources,
            };
          },
        ),
    );
  } finally {
    release();
    producer.closeAllConnections();
    await new Promise<void>((resolve, reject) =>
      producer.close((error) => (error ? reject(error) : resolve())),
    );
  }
});
