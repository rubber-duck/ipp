import assert from "node:assert/strict";
import { alias, entity } from "../driver.js";
import { aliasEntity, committed, observed } from "../assertions.js";
import type { ScenarioContext } from "../environment.js";

export async function scalarBaseAndEffectiveValues(
  context: ScenarioContext,
): Promise<void> {
  const creation = committed(
    await context.execute(
      "create scalar driver fixture",
      { batchId: 20n },
      () =>
        context.driver.submit(
          20n,
          [
            { kind: "create", alias: "source", symbolicId: "driver-source" },
            { kind: "insertScalar", entity: alias("source"), value: 4 },
            { kind: "create", alias: "target", symbolicId: "driver-target" },
            { kind: "insertScalar", entity: alias("target"), value: 99 },
            {
              kind: "insertLinearDriver",
              entity: alias("target"),
              source: alias("source"),
              scale: 2,
              bias: 1,
            },
          ],
          { signal: context.signal },
        ),
    ),
  );
  const source = aliasEntity(creation, "source");
  const target = aliasEntity(creation, "target");

  const first = observed(
    await context.execute("inspect first evaluation", target, () =>
      context.driver.inspect(target, { signal: context.signal }),
    ),
  );
  assert.deepEqual(first.scalar, { base: 99, effective: 9 });
  assert.deepEqual(first.linearDriver, {
    source,
    scale: 2,
    bias: 1,
  });

  const update = committed(
    await context.execute("update source base", { batchId: 21n, source }, () =>
      context.driver.submit(
        21n,
        [{ kind: "setScalar", entity: entity(source), value: 6 }],
        { signal: context.signal },
      ),
    ),
  );
  assert.ok(update.commitTick > creation.commitTick);
  assert.deepEqual(
    observed(
      await context.execute("inspect second evaluation", target, () =>
        context.driver.inspect(target, { signal: context.signal }),
      ),
    ).scalar,
    { base: 99, effective: 13 },
  );
}

export async function sourceDeletionDoesNotReconnect(
  context: ScenarioContext,
): Promise<void> {
  const creation = committed(
    await context.execute("create deletion fixture", { batchId: 30n }, () =>
      context.driver.submit(
        30n,
        [
          { kind: "create", alias: "source" },
          { kind: "insertScalar", entity: alias("source"), value: 8 },
          { kind: "create", alias: "target" },
          { kind: "insertScalar", entity: alias("target"), value: 40 },
          {
            kind: "insertLinearDriver",
            entity: alias("target"),
            source: alias("source"),
            scale: 3,
            bias: -2,
          },
        ],
        { signal: context.signal },
      ),
    ),
  );
  const source = aliasEntity(creation, "source");
  const target = aliasEntity(creation, "target");
  assert.equal(
    observed(
      await context.execute("inspect before source deletion", target, () =>
        context.driver.inspect(target, { signal: context.signal }),
      ),
    ).scalar?.effective,
    22,
  );

  const deletion = committed(
    await context.execute(
      "delete driver source",
      { batchId: 31n, source },
      () =>
        context.driver.submit(
          31n,
          [{ kind: "delete", entity: entity(source) }],
          { signal: context.signal },
        ),
    ),
  );
  assert.ok(deletion.commitTick > creation.commitTick);
  const invalidated = observed(
    await context.execute("inspect invalidated driver", target, () =>
      context.driver.inspect(target, { signal: context.signal }),
    ),
  );
  assert.deepEqual(invalidated.linearDriver, {
    source,
    scale: 3,
    bias: -2,
  });
  assert.deepEqual(invalidated.scalar, { base: 40, effective: 40 });

  const replacementOutcome = committed(
    await context.execute("reuse deleted source slot", { batchId: 32n }, () =>
      context.driver.submit(
        32n,
        [
          { kind: "create", alias: "replacement" },
          { kind: "insertScalar", entity: alias("replacement"), value: 100 },
        ],
        { signal: context.signal },
      ),
    ),
  );
  const replacement = aliasEntity(replacementOutcome, "replacement");
  assert.equal(replacement.index, source.index);
  assert.notEqual(replacement.generation, source.generation);
  assert.ok(replacementOutcome.commitTick > deletion.commitTick);
  const afterReuse = observed(
    await context.execute("inspect driver after source reuse", target, () =>
      context.driver.inspect(target, { signal: context.signal }),
    ),
  );
  assert.deepEqual(afterReuse.linearDriver, {
    source,
    scale: 3,
    bias: -2,
  });
  assert.deepEqual(afterReuse.scalar, { base: 40, effective: 40 });
}
