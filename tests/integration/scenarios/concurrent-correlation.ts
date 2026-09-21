import assert from "node:assert/strict";
import { alias } from "../driver.js";
import { aliasEntity, committed } from "../assertions.js";
import type { ScenarioContext } from "../environment.js";

const CONCURRENT_BATCHES = 32;

export async function concurrentRpcResponsesStayCorrelated(
  context: ScenarioContext,
): Promise<void> {
  const seed = committed(
    await context.execute(
      "create concurrent RPC marker",
      { batchId: 900n },
      () =>
        context.driver.submit(
          900n,
          [
            {
              kind: "create",
              alias: "marker",
              symbolicId: "concurrent-rpc-marker",
            },
            { kind: "insertScalar", entity: alias("marker"), value: 64 },
          ],
          { signal: context.signal },
        ),
    ),
  );
  const marker = aliasEntity(seed, "marker");
  const batchIds = Array.from(
    { length: CONCURRENT_BATCHES },
    (_value, index) => 1_000n + BigInt(index),
  );

  const correlation = await context.execute(
    "correlate 64 concurrent batch and inspection RPCs",
    { batches: batchIds.length, inspections: batchIds.length },
    () =>
      context.driver.correlateConcurrentRequests(marker, batchIds, {
        signal: context.signal,
      }),
  );
  assert.equal(correlation.batches.length, CONCURRENT_BATCHES);
  assert.equal(correlation.inspections.length, CONCURRENT_BATCHES);

  for (let index = 0; index < correlation.batches.length; index += 1) {
    const batch = correlation.batches[index];
    const expectedId = batchIds[index];
    assert.ok(batch);
    assert.ok(expectedId !== undefined);
    assert.equal(batch.requestedBatchId, expectedId);
    assert.equal(batch.returnedBatchId, expectedId);
    assert.ok(batch.commitTick > seed.commitTick);
    assert.equal(batch.aliasCount, 1);
  }
  for (const inspection of correlation.inspections) {
    assert.ok(inspection.tick > seed.commitTick);
    assert.ok(Number.isFinite(inspection.time));
    assert.ok(inspection.time >= 0);
    assert.equal(inspection.sawMarker, true);
  }
}
