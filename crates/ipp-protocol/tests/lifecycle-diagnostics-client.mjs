import assert from "node:assert/strict";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";

function integer(value) {
  const bytes = new Uint8Array(8);
  new DataView(bytes.buffer).setBigUint64(0, value, true);
  return bytes;
}

const join = (...parts) => Uint8Array.from(parts.flatMap((part) => [...part]));
const world = { id: 3n, incarnation: 5n };

for (const target of ["native", "wasm"]) {
  const codec = await import(
    pathToFileURL(resolve(`target/world-host-build/${target}/generated.js`))
      .href
  );
  test(`${target} diagnostic exchange and independent counter bytes`, () => {
    const query = {
      session: 7n,
      requestId: 2n,
      body: { kind: "lifecycleDiagnostics", query: { world, output: 9n } },
    };
    assert.deepEqual(
      codec.encodeRequest(query),
      join(
        integer(7n),
        integer(2n),
        [36],
        integer(3n),
        integer(5n),
        integer(9n),
      ),
    );
    for (const endpoint of [
      { world, output: 0n },
      { world: { ...world, id: 0n }, output: 9n },
      { world: { ...world, incarnation: 0n }, output: 9n },
    ])
      assert.throws(() =>
        codec.encodeRequest({
          ...query,
          body: { kind: "lifecycleDiagnostics", query: endpoint },
        }),
      );
    const bytes = join(
      integer(7n),
      integer(2n),
      integer(0n),
      [38],
      integer(3n),
      integer(5n),
      integer(9n),
      integer(99n),
      integer(12n),
      [0],
      integer(2n),
      integer(1024n),
      [1],
    );
    assert.deepEqual(codec.decodeResponse(bytes, 7n), {
      session: 7n,
      requestId: 2n,
      tick: 0n,
      body: {
        kind: "lifecycleDiagnostics",
        sample: {
          world,
          output: 9n,
          work: { lookups: 99n, recipientVisits: 12n, saturated: false },
          traffic: { queuedEvents: 2n, queuedBytes: 1024n, saturated: true },
        },
      },
    });
    for (let length = 0; length < bytes.length; length++)
      assert.throws(() => codec.decodeResponse(bytes.subarray(0, length), 7n));
    for (const offset of [65, 82]) {
      const invalid = bytes.slice();
      invalid[offset] = 2;
      assert.throws(() => codec.decodeResponse(invalid, 7n));
    }
    for (const offset of [25, 33, 41]) {
      const invalid = bytes.slice();
      invalid.fill(0, offset, offset + 8);
      assert.throws(() => codec.decodeResponse(invalid, 7n));
    }
    for (const offset of [8, 16]) {
      const invalid = bytes.slice();
      invalid[offset] = offset === 8 ? 0 : 1;
      assert.throws(() => codec.decodeResponse(invalid, 7n));
    }
    assert.throws(() => codec.decodeResponse(join(bytes, [0]), 7n));
    assert.throws(() => codec.decodeResponse(bytes, 8n));
    // The exchange is reachable only through `@ipp/client/diagnostics`.
    assert.equal("lifecycleStatistics" in codec.IppClient.prototype, false);
  });
}
