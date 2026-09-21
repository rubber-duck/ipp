import assert from "node:assert/strict";
import test from "node:test";
import { setImmediate } from "node:timers/promises";
import { ClientAssetSources, clientAssetSource } from "../src/asset-sources.js";
import {
  applyCommandPages,
  CommandEncodingError,
} from "../src/command-pages.js";
import { HostWireWriter } from "../src/host-protocol.js";
import type { BatchOutcome, Command, Request } from "../src/types.js";

function harness() {
  const sent: Uint8Array[] = [];
  const sources = new ClientAssetSources(
    () => 7n,
    (bytes) => sent.push(bytes),
    1000,
    (error) => sources.close(error),
  );
  const reply = (frame: Uint8Array, error?: string) => {
    const writer = new HostWireWriter();
    writer.raw(Uint8Array.of(73, 80, 65, 82));
    writer.u64(7n);
    writer.u64(
      new DataView(frame.buffer, frame.byteOffset).getBigUint64(12, true),
    );
    writer.u8(error ? 1 : 0);
    if (error) writer.string(error);
    sources.receive(writer.finish());
  };
  return { sources, sent, reply };
}

test("source delivery snapshots input and pipelines a bounded window outside command framing", async () => {
  const { sources, sent, reply } = harness();
  const input = new Uint8Array(10 * 65536 + 13).fill(42);
  const source = clientAssetSource(7n, 10, "immutable");
  const delivered = sources.register(source, input.buffer);
  input.fill(0);
  source.source = "changed after submission";
  await setImmediate();
  assert.equal(sent.length, 1);
  assert.ok(new TextDecoder().decode(sent[0]).includes("#immutable"));
  reply(sent[0]!);
  await setImmediate();
  assert.equal(sent.length, 9);
  assert.ok(sent.every((frame) => frame.length <= 65536 + 128));
  for (const frame of sent.slice(1)) {
    assert.equal(frame[20], 1);
    assert.ok(frame.subarray(41).every((byte) => byte === 42));
    reply(frame);
  }
  await setImmediate();
  assert.equal(sent.length, 12);
  for (const frame of sent.slice(9)) reply(frame);
  await setImmediate();
  assert.equal(sent[12]![20], 2);
  reply(sent[12]!);
  await delivered;
  sources.close(new Error("closed"));
});

test("failed delivery drains submitted chunks before cancelling staging", async () => {
  const { sources, sent, reply } = harness();
  const delivered = sources.register(
    clientAssetSource(7n, 10, "failed"),
    new ArrayBuffer(10 * 65536),
  );
  const rejection = assert.rejects(delivered, /provider rejected/);
  await setImmediate();
  reply(sent[0]!);
  await setImmediate();
  reply(sent[1]!, "provider rejected");
  await setImmediate();
  assert.equal(sent.length, 9, "cancel must wait for in-flight results");
  for (const frame of sent.slice(2)) reply(frame);
  await setImmediate();
  assert.equal(sent.length, 10);
  assert.equal(sent[9]![20], 3);
  reply(sent[9]!);
  await rejection;
  sources.close(new Error("closed"));
});

test("release remains ordered behind publication and its submitted chunks", async () => {
  const { sources, sent, reply } = harness();
  const source = clientAssetSource(7n, 10, "ordered-release");
  const registered = sources.register(source, new ArrayBuffer(1));
  const released = sources.release(source);
  await setImmediate();
  assert.equal(sent.length, 1);
  reply(sent[0]!); // begin
  await setImmediate();
  assert.equal(sent.length, 2);
  reply(sent[1]!); // only chunk
  await setImmediate();
  assert.equal(sent.length, 3);
  reply(sent[2]!); // end
  await registered;
  await setImmediate();
  assert.equal(sent.length, 4);
  assert.equal(sent[3]![20], 4, "release was not serialized after delivery");
  reply(sent[3]!);
  await released;
  sources.close(new Error("closed"));
});

test("source ownership accepts opaque session namespaces", async () => {
  const { sources, sent, reply } = harness();
  const source = {
    kind: 5,
    source: "client://7/react-scope/mesh#revision",
    variant: 3,
  };
  const released = sources.release(source);
  await setImmediate();
  assert.equal(sent.length, 1);
  assert.ok(new TextDecoder().decode(sent[0]).includes(source.source));
  reply(sent[0]!);
  await released;

  assert.throws(
    () =>
      sources.release({
        ...source,
        source: "client://8/react-scope/mesh#revision",
      }),
    /does not belong to this client session/,
  );
  assert.throws(
    () => sources.release({ ...source, source: "client://7/react-scope/mesh" }),
    /does not belong to this client session/,
  );
  sources.close(new Error("closed"));
});

function command(alias: number): Command {
  return {
    kind: "create",
    alias,
    metadata: { symbolicId: `entity-${alias}`, classes: [] },
  };
}

function encodePage(request: Request): Uint8Array<ArrayBuffer> {
  if (request.body.kind !== "batchChunk")
    throw new Error("expected command page");
  return new Uint8Array(32 + request.body.batch.operations.length * 16);
}

function successfulPage(id: bigint, operations: Command[]): BatchOutcome {
  return {
    ok: true,
    batchId: id,
    tick: 3n,
    aliases: operations.flatMap((operation) =>
      operation.kind === "create"
        ? [{ alias: operation.alias, id: BigInt(1000 + operation.alias) }]
        : [],
    ),
    stateOverlays: [],
  };
}

test("paged outcomes preserve global failure offsets and every prior identity", async () => {
  const pages: number[] = [];
  const outcome = await applyCommandPages(
    {
      async beginBatch() {
        return 91n;
      },
      async batchChunk(id, operations) {
        pages.push(operations.length);
        if (pages.length === 2)
          return {
            ...successfulPage(id, operations.slice(0, 3)),
            ok: false,
            error: { scope: "operation", operation: 3, reason: "fixture" },
          };
        return successfulPage(id, operations);
      },
      async endBatch() {
        throw new Error("failed batch must not be terminated");
      },
    },
    Array.from({ length: 300 }, (_, index) => command(index + 1)),
    encodePage,
  );
  assert.deepEqual(pages, [256, 44]);
  assert.equal(outcome.ok, false);
  if (outcome.ok) return;
  assert.equal(outcome.error.operation, 259);
  assert.equal(outcome.aliases.length, 259);
});

test("local iterable failure drains pages, terminates the gate, and exposes identities", async () => {
  let ended = false;
  function* commands(): Generator<Command> {
    for (let alias = 1; alias <= 260; alias++) yield command(alias);
    throw new Error("local producer failed");
  }
  const rejected = applyCommandPages(
    {
      async beginBatch() {
        return 92n;
      },
      async batchChunk(id, operations) {
        return successfulPage(id, operations);
      },
      async endBatch(id) {
        assert.equal(id, 92n);
        ended = true;
      },
    },
    commands(),
    encodePage,
  );
  await assert.rejects(rejected, (error: unknown) => {
    assert.ok(error instanceof CommandEncodingError);
    assert.match(error.message, /local producer failed/);
    assert.equal(error.aliases.size, 256);
    return true;
  });
  assert.equal(ended, true);
});
