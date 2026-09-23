import assert from "node:assert/strict";
import test from "node:test";
import { workerTransport } from "../src/worker.js";

test("worker transport requires the target contract's message budget", () => {
  for (const maxMessageBytes of [0, -1, 0.5, Number.NaN])
    assert.throws(
      () => workerTransport("worker.js", "runtime.wasm", maxMessageBytes),
      /maxMessageBytes must be the target contract's positive message budget/,
    );
});

test("worker asset cache bytes stay within the WASM u32 boundary", () => {
  for (const assetCacheBytes of [-1, 0.5, 0x1_0000_0000, Number.NaN]) {
    assert.throws(
      () =>
        workerTransport("worker.js", "runtime.wasm", 1_048_576, {
          assetCacheBytes,
        }),
      /assetCacheBytes must be an integer in \[0, 4294967295\]/,
    );
  }
});
