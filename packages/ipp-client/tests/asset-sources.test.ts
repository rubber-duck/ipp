import assert from "node:assert/strict";
import test from "node:test";
import { setImmediate } from "node:timers/promises";
import { ClientAssetSources, clientAssetSource } from "../src/asset-sources.js";
import {
  BatchIdentities,
  type CommandBatchSink,
  CommandPageCodec,
  encodeCommandPages,
  openCommandBatch,
  planCommandPages,
  submitCommandPages,
} from "../src/command-pages.js";
import { HostWireWriter } from "../src/host-protocol.js";
import type { BatchOutcome, Command, Request } from "../src/types.js";

/** The contract's page bounds as generated for every target. */
const PAGE_LIMITS = { commands: 1024, bytes: 256 * 1024 } as const;

function harness() {
  const sent: Uint8Array[] = [];
  const sources = new ClientAssetSources(
    () => 7n,
    (bytes) => {
      sent.push(bytes);
      return undefined;
    },
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

let encodedCommands = 0;

/** A page is a 32-byte header ending in its u32 command count, then 16 bytes per command. */
function encodePage(request: Request): Uint8Array<ArrayBuffer> {
  if (request.body.kind !== "submitBatch")
    throw new Error("expected command page");
  const bytes = new Uint8Array(32 + request.body.operations.length * 16);
  const view = new DataView(bytes.buffer);
  view.setBigUint64(0, request.session, true);
  view.setBigUint64(8, request.requestId, true);
  view.setUint32(16, request.body.batchId, true);
  bytes[20] = request.body.last ? 1 : 0;
  view.setUint32(28, request.body.operations.length, true);
  request.body.operations.forEach((operation, index) => {
    if (operation.kind === "delete") throw new Error("unencodable command");
    if (operation.kind === "create")
      view.setUint32(32 + 16 * index, operation.alias, true);
    encodedCommands++;
  });
  return bytes;
}

function codec() {
  return new CommandPageCodec(encodePage, PAGE_LIMITS);
}

function outcome(batchId: number): BatchOutcome {
  return {
    ok: true,
    batchId: BigInt(batchId),
    tick: 3n,
    aliases: [],
    symbols: [],
    effects: [],
  };
}

/** Records pages in handoff order; the final reply resolves when released. */
function recordingSink() {
  const sent: { batchId: number; last: boolean; count: number }[] = [];
  let release!: () => void;
  const released = new Promise<void>((resolve) => {
    release = resolve;
  });
  const sink: CommandBatchSink = {
    page(batchId, page) {
      sent.push({ batchId, last: false, count: page.operations.length });
    },
    async finish(batchId, page) {
      sent.push({ batchId, last: true, count: page.operations.length });
      await released;
      return outcome(batchId);
    },
  };
  return { sent, sink, release };
}

test("batch identities wrap as a u32 counter and skip identities still open", () => {
  const identities = new BatchIdentities();
  const restart = () => {
    (identities as unknown as { next: number }).next = 0xffff_fffe;
  };
  restart();
  const held = identities.allocate();
  assert.equal(held, 0xffff_fffe);
  const last = identities.allocate();
  assert.equal(last, 0xffff_ffff);
  assert.equal(identities.allocate(), 0, "the counter wraps");
  identities.release(last);
  identities.release(0);
  restart();
  assert.equal(
    identities.allocate(),
    0xffff_ffff,
    "an open identity is skipped",
  );
  identities.release(held);
  restart();
  assert.equal(
    identities.allocate(),
    0xffff_fffe,
    "a finished identity is reusable",
  );
});

test("pages leave back to back and only the final page is awaited", async () => {
  const identities = new BatchIdentities();
  const { sent, sink, release } = recordingSink();
  const pages = encodeCommandPages(
    Array.from({ length: 2 * PAGE_LIMITS.commands + 3 }, (_, index) =>
      command(index + 1),
    ),
    codec(),
  );
  const result = submitCommandPages(identities, sink, pages);
  assert.deepEqual(sent, [
    { batchId: 0, last: false, count: PAGE_LIMITS.commands },
    { batchId: 0, last: false, count: PAGE_LIMITS.commands },
    { batchId: 0, last: true, count: 3 },
  ]);
  assert.equal(identities.allocate(), 1, "the identity follows the final page");
  release();
  assert.equal((await result).batchId, 0n);
});

test("an empty batch is one final page", async () => {
  const { sent, sink, release } = recordingSink();
  release();
  await submitCommandPages(
    new BatchIdentities(),
    sink,
    encodeCommandPages([], codec()),
  );
  assert.deepEqual(sent, [{ batchId: 0, last: true, count: 0 }]);
});

test("an unencodable command fails planning before any page leaves", () => {
  assert.throws(
    () =>
      planCommandPages(
        [command(1), { kind: "delete", entity: { kind: "alias", alias: 1 } }],
        encodePage,
        PAGE_LIMITS,
      ),
    /unencodable command/,
  );
});

test("a streaming writer sends full pages as they fill and finishes once", async () => {
  const identities = new BatchIdentities();
  const { sent, sink, release } = recordingSink();
  const writer = openCommandBatch(identities, sink, codec());
  writer.write(
    Array.from({ length: PAGE_LIMITS.commands }, (_, index) =>
      command(index + 1),
    ),
  );
  assert.deepEqual(sent, [], "a full page waits for the next command");
  writer.write([command(999)]);
  assert.deepEqual(sent, [
    { batchId: 0, last: false, count: PAGE_LIMITS.commands },
  ]);
  const result = writer.finish();
  assert.throws(() => writer.write([command(1)]), /finished/);
  await assert.rejects(writer.finish(), /finished/);
  release();
  assert.equal((await result).batchId, 0n);
  assert.deepEqual(sent.at(-1), { batchId: 0, last: true, count: 1 });
});

test("a failed writer never sends its final page and keeps its identity", async () => {
  const identities = new BatchIdentities();
  const { sent, sink } = recordingSink();
  const writer = openCommandBatch(identities, sink, codec());
  assert.throws(
    () =>
      writer.write([
        command(1),
        { kind: "delete", entity: { kind: "alias", alias: 1 } },
      ]),
    /unencodable command/,
  );
  await assert.rejects(writer.finish(), /failed/);
  assert.deepEqual(sent, []);
  assert.equal(identities.allocate(), 1);
});

test("each command is encoded once and pages are assembled from those bytes", () => {
  const operations = Array.from(
    { length: PAGE_LIMITS.commands + 5 },
    (_, index) => command(index + 1),
  );
  const pages = codec();
  encodedCommands = 0;
  const encoded = encodeCommandPages(operations, pages);
  assert.equal(encodedCommands, operations.length);
  const messages = encoded.map((page, index) =>
    pages.message(
      9n,
      index === encoded.length - 1 ? 4n : 0n,
      7,
      index === encoded.length - 1,
      page,
    ),
  );
  assert.equal(
    encodedCommands,
    operations.length,
    "assembly re-encodes nothing",
  );
  assert.deepEqual(
    messages,
    encoded.map((page, index) =>
      encodePage({
        session: 9n,
        requestId: index === encoded.length - 1 ? 4n : 0n,
        body: {
          kind: "submitBatch",
          batchId: 7,
          last: index === encoded.length - 1,
          operations: page.operations,
        },
      }),
    ),
    "assembled pages equal the target's own page encoding",
  );
});

test("a target whose pages are not a counted header refuses to page", () => {
  const unpaged = new CommandPageCodec(
    (request) =>
      new Uint8Array(
        32 +
          (request.body.kind === "submitBatch"
            ? request.body.operations.length * 16
            : 0),
      ),
    PAGE_LIMITS,
  );
  assert.throws(
    () => encodeCommandPages([command(1)], unpaged),
    /not a counted header followed by commands/,
  );
});

test("page planning reports the commands of each page", () => {
  assert.deepEqual(
    planCommandPages(
      Array.from({ length: PAGE_LIMITS.commands + 1 }, (_, index) =>
        command(index + 1),
      ),
      encodePage,
      PAGE_LIMITS,
    ).map((page) => page.length),
    [PAGE_LIMITS.commands, 1],
  );
});
