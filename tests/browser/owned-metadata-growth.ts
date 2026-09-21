import assert from "node:assert/strict";
import { entity } from "../integration/driver.js";
import { aliasEntity, committed, observed } from "../integration/assertions.js";
import type { ScenarioContext } from "../integration/environment.js";

const ENTITY_COUNT = 70;
const ENTITIES_PER_BATCH = 10;
const UTF8_REPETITIONS = 950;

export async function ownedMetadataSurvivesWasmGrowth(
  context: ScenarioContext,
): Promise<void> {
  const values = Array.from({ length: ENTITY_COUNT }, (_value, index) =>
    largeUtf8("retained", index),
  );
  const entities = [];
  for (let start = 0; start < ENTITY_COUNT; start += ENTITIES_PER_BATCH) {
    const batchId = BigInt(50 + start / ENTITIES_PER_BATCH);
    const operations = values
      .slice(start, start + ENTITIES_PER_BATCH)
      .map((symbolicId, offset) => ({
        kind: "create" as const,
        alias: `retained-${start + offset}`,
        symbolicId,
        classes: [`group-${start / ENTITIES_PER_BATCH}-🦆`],
      }));
    const outcome = committed(
      await context.execute(
        "grow retained owned metadata",
        { batchId, firstEntity: start, entities: operations.length },
        () =>
          context.driver.submit(batchId, operations, {
            signal: context.signal,
          }),
      ),
    );
    for (let offset = 0; offset < operations.length; offset += 1) {
      entities.push(aliasEntity(outcome, `retained-${start + offset}`));
    }
    const latest = entities.at(-1);
    assert.ok(latest);
    assert.equal(
      observed(
        await context.execute(
          "decode inspection after retained growth",
          { entity: latest, retained: entities.length },
          () => context.driver.inspect(latest, { signal: context.signal }),
        ),
      ).symbolicId,
      values[entities.length - 1],
    );
  }

  const replacement = entities.at(-1);
  assert.ok(replacement);

  for (let revision = 1; revision <= 4; revision += 1) {
    const current = largeUtf8("replacement", revision);
    committed(
      await context.execute(
        "replace large owned metadata",
        { batchId: BigInt(100 + revision), revision },
        () =>
          context.driver.submit(
            BigInt(100 + revision),
            [
              {
                kind: "updateMetadata",
                entity: entity(replacement),
                symbolicId: current,
                classes: [`revision-${revision}-é`],
              },
            ],
            { signal: context.signal },
          ),
      ),
    );
    const observation = observed(
      await context.execute(
        "inspect current metadata response",
        { replacement, revision },
        () => context.driver.inspect(replacement, { signal: context.signal }),
      ),
    );
    assert.equal(observation.symbolicId, current);
    assert.deepEqual(observation.classes, [`revision-${revision}-é`]);
  }

  for (const retainedIndex of [0, Math.floor(ENTITY_COUNT / 2)]) {
    const retainedEntity = entities[retainedIndex];
    assert.ok(retainedEntity);
    const retained = observed(
      await context.execute(
        "reinspect earlier retained metadata",
        { retainedEntity, retainedIndex },
        () =>
          context.driver.inspect(retainedEntity, { signal: context.signal }),
      ),
    );
    assert.equal(retained.symbolicId, values[retainedIndex]);
    assert.deepEqual(retained.classes, [
      `group-${Math.floor(retainedIndex / ENTITIES_PER_BATCH)}-🦆`,
    ]);
  }
}

function largeUtf8(label: string, revision: number): string {
  return `${label}:${revision}:` + "🦆é".repeat(UTF8_REPETITIONS);
}
