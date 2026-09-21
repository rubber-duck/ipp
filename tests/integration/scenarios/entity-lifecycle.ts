import assert from "node:assert/strict";
import { alias, entity } from "../driver.js";
import { aliasEntity, committed, observed, rejectedAt } from "../assertions.js";
import type { ScenarioContext } from "../environment.js";

export async function metadataAndGenerationReuse(
  context: ScenarioContext,
): Promise<void> {
  const firstOutcome = committed(
    await context.execute("create first generation", { batchId: 10n }, () =>
      context.driver.submit(
        10n,
        [
          {
            kind: "create",
            alias: "first",
            symbolicId: "first-name",
            classes: ["red", "selected"],
          },
          { kind: "insertScalar", entity: alias("first"), value: 3 },
        ],
        { signal: context.signal },
      ),
    ),
  );
  const first = aliasEntity(firstOutcome, "first");
  const initial = observed(
    await context.execute("inspect initial metadata", first, () =>
      context.driver.inspect(first, { signal: context.signal }),
    ),
  );
  assert.equal(initial.symbolicId, "first-name");
  assert.deepEqual(initial.classes, ["red", "selected"]);

  committed(
    await context.execute("update metadata", { batchId: 11n, first }, () =>
      context.driver.submit(
        11n,
        [
          {
            kind: "updateMetadata",
            entity: entity(first),
            symbolicId: "renamed",
            classes: ["blue"],
          },
        ],
        { signal: context.signal },
      ),
    ),
  );
  const renamed = observed(
    await context.execute("find renamed metadata", "renamed", () =>
      context.driver.findBySymbolicId("renamed", { signal: context.signal }),
    ),
  );
  assert.equal(renamed.entity.bits, first.bits);
  assert.deepEqual(renamed.classes, ["blue"]);
  assert.equal(
    await context.execute("old metadata index removed", "first-name", () =>
      context.driver.findBySymbolicId("first-name", { signal: context.signal }),
    ),
    null,
  );

  committed(
    await context.execute(
      "delete first generation",
      { batchId: 12n, first },
      () =>
        context.driver.submit(
          12n,
          [{ kind: "delete", entity: entity(first) }],
          { signal: context.signal },
        ),
    ),
  );
  assert.equal(
    await context.execute("inspect deleted generation", first, () =>
      context.driver.inspect(first, { signal: context.signal }),
    ),
    null,
  );

  const secondOutcome = committed(
    await context.execute("reuse entity slot", { batchId: 13n }, () =>
      context.driver.submit(
        13n,
        [{ kind: "create", alias: "second", symbolicId: "second-name" }],
        { signal: context.signal },
      ),
    ),
  );
  const second = aliasEntity(secondOutcome, "second");
  assert.equal(
    second.index,
    first.index,
    "fresh world should reuse the free slot",
  );
  assert.notEqual(second.generation, first.generation);
  assert.notEqual(second.bits, first.bits);

  const staleWrite = await context.execute(
    "reject stale generation",
    { batchId: 14n, first },
    () =>
      context.driver.submit(
        14n,
        [{ kind: "insertScalar", entity: entity(first), value: 91 }],
        { signal: context.signal },
      ),
  );
  rejectedAt(staleWrite, 0);
  assert.equal(
    observed(
      await context.execute("inspect reused generation", second, () =>
        context.driver.inspect(second, { signal: context.signal }),
      ),
    ).scalar,
    null,
  );
}
