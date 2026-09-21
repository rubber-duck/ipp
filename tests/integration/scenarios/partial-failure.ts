import assert from "node:assert/strict";
import { alias, entity } from "../driver.js";
import { aliasEntity, committed, observed, rejectedAt } from "../assertions.js";
import type { ScenarioContext } from "../environment.js";

export async function partialFailureThenSuccess(
  context: ScenarioContext,
): Promise<void> {
  const staleCreation = committed(
    await context.execute("create stale fixture", { batchId: 1n }, () =>
      context.driver.submit(
        1n,
        [
          { kind: "create", alias: "stale", symbolicId: "stale-fixture" },
          { kind: "insertScalar", entity: alias("stale"), value: 5 },
        ],
        { signal: context.signal },
      ),
    ),
  );
  const stale = aliasEntity(staleCreation, "stale");
  committed(
    await context.execute("delete stale fixture", { batchId: 2n, stale }, () =>
      context.driver.submit(2n, [{ kind: "delete", entity: entity(stale) }], {
        signal: context.signal,
      }),
    ),
  );

  const failed = await context.execute(
    "fail after applied mutations",
    { batchId: 3n, stale },
    () =>
      context.driver.submit(
        3n,
        [
          {
            kind: "create",
            alias: "partial",
            symbolicId: "partial-entity",
            classes: ["partial"],
          },
          { kind: "insertScalar", entity: alias("partial"), value: 12 },
          { kind: "setScalar", entity: entity(stale), value: 99 },
        ],
        { signal: context.signal },
      ),
  );
  rejectedAt(failed, 2);
  assert.equal(failed.status, "rejected");
  if (failed.status !== "rejected") throw new Error("expected failed batch");
  const partial = observed(
    await context.driver.findBySymbolicId("partial-entity", {
      signal: context.signal,
    }),
  );
  assert.deepEqual(partial.entity, failed.aliases.partial);
  assert.deepEqual(partial.scalar, { base: 12, effective: 12 });
  committed(
    await context.driver.submit(
      31n,
      [{ kind: "setScalar", entity: entity(partial.entity), value: 13 }],
      { signal: context.signal },
    ),
  );
  assert.deepEqual(
    observed(
      await context.driver.inspect(partial.entity, { signal: context.signal }),
    ).scalar,
    { base: 13, effective: 13 },
  );

  const success = committed(
    await context.execute("submit after rejection", { batchId: 4n }, () =>
      context.driver.submit(
        4n,
        [
          {
            kind: "create",
            alias: "survivor",
            symbolicId: "after-rejection",
          },
          { kind: "insertScalar", entity: alias("survivor"), value: 7 },
        ],
        { signal: context.signal },
      ),
    ),
  );
  const survivor = aliasEntity(success, "survivor");
  const observation = observed(
    await context.execute("inspect successful continuation", survivor, () =>
      context.driver.inspect(survivor, { signal: context.signal }),
    ),
  );
  assert.equal(observation.symbolicId, "after-rejection");
  assert.deepEqual(observation.scalar, { base: 7, effective: 7 });
}
