import assert from "node:assert/strict";
import test from "node:test";
import { SharedBufferProducer } from "../src/buffer-source.js";

test("shared publication exhaustion never wraps or invokes another writer", () => {
  const producer = new SharedBufferProducer(4);
  const first = producer.publish((bytes) => bytes.fill(17));
  const state = new Int32Array(first.control);
  // This producer fixture releases its own publication before changing its
  // identity counter; no Host copy/read lease exists in this boundary test.
  Atomics.store(state, 0, 0);
  Atomics.store(state, 1, 0x7ffffffe);
  let calls = 0;
  const last = producer.publish((bytes) => {
    calls++;
    bytes.fill(29);
  });
  assert.equal(last.generation, 0x7fffffff);
  Atomics.store(state, 0, 0);
  for (let attempt = 0; attempt < 3; attempt++) {
    assert.throws(
      () =>
        producer.publish(() => {
          calls++;
        }),
      /identities exhausted/,
    );
    assert.equal(calls, 1);
    assert.equal(Atomics.load(state, 1), 0x7fffffff);
    assert.equal(producer.released, true);
    assert.deepEqual([...new Uint8Array(last.buffer)], [29, 29, 29, 29]);
  }
});
