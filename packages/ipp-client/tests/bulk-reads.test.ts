import assert from "node:assert/strict";
import test from "node:test";
import { BulkReadClient } from "../src/bulk-reads.js";
import { HostWireReader, HostWireWriter } from "../src/host-protocol.js";

function reader(length: number) {
  const sent: Uint8Array[] = [];
  const client = new BulkReadClient(
    () => 7n,
    (request) => {
      sent.push(request);
      const input = new HostWireReader(request);
      input.raw(4);
      input.u64();
      const id = input.u64();
      const read = input.u64();
      const operation = input.u8();
      const output = new HostWireWriter();
      output.raw(Uint8Array.of(73, 80, 68, 83));
      output.u64(7n);
      output.u64(id);
      output.u64(read);
      if (operation === 0) {
        const offset = Number(input.u64());
        const count = Math.min(65536, length - offset);
        output.u8(0);
        output.u64(BigInt(offset));
        output.u8(offset + count === length ? 1 : 0);
        output.bytes(new Uint8Array(count).fill(offset / 65536 + 1));
      } else output.u8(1);
      queueMicrotask(() => client.receive(output.finish()));
    },
  );
  return {
    client,
    sent,
    descriptor: {
      reference: { connection: 7n, read: 3n },
      length: BigInt(length),
    },
  };
}

test("shared reader pipelines only eight chunks and acknowledges EOF after the final yielded window is consumed", async () => {
  const { client, sent, descriptor } = reader(10 * 65536 + 3);
  const iterator = client.chunks(descriptor);
  const first = await iterator.next();
  assert.equal(first.value?.length, 65536);
  assert.equal(sent.length, 8);
  assert.ok(sent.every((request) => request[28] === 0));
  let bytes = first.value!.length;
  for await (const chunk of iterator) bytes += chunk.length;
  assert.equal(bytes, Number(descriptor.length));
  const final = sent.at(-1)!;
  assert.equal(final[28], 1);
  assert.equal(final[37], 1);
  assert.equal(sent.filter((request) => request[28] === 2).length, 0);
  client.close(new Error("done"));
});

test("returning a window iterator abandons unread content and rejects foreign connection references", async () => {
  const { client, sent, descriptor } = reader(65537);
  const iterator = client.chunks(descriptor);
  await iterator.next();
  await iterator.return(undefined);
  assert.equal(sent.at(-1)![28], 2);
  await assert.rejects(
    client.release({ connection: 8n, read: 3n }),
    /another connection/,
  );
  client.close(new Error("done"));
});

test("unknown totals stream until EOF and local allocation limits release unread ownership", async () => {
  const { client, sent, descriptor } = reader(65539);
  const bytes = await client.readAll(
    { reference: descriptor.reference },
    { maxBytes: 65539 },
  );
  assert.equal(bytes.length, 65539);
  assert.equal(bytes[0], 1);
  assert.equal(bytes[65536], 2);
  assert.equal(sent.filter((request) => request[28] === 0).length, 2);
  await assert.rejects(
    client.readAll(descriptor, { maxBytes: 1 }),
    /byte budget/,
  );
  assert.equal(sent.at(-1)![28], 2);
  client.close(new Error("done"));
});
