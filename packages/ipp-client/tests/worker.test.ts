import assert from "node:assert/strict";
import test from "node:test";
import { workerTransport } from "../src/worker.js";

test("worker asset cache bytes stay within the WASM u32 boundary", () => {
  for (const assetCacheBytes of [-1, 0.5, 0x1_0000_0000, Number.NaN]) {
    assert.throws(
      () => workerTransport("worker.js", "runtime.wasm", { assetCacheBytes }),
      /assetCacheBytes must be an integer in \[0, 4294967295\]/,
    );
  }
});
