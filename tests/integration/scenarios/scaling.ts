/** Scene assertions shared by real native and worker transports. Hosts own all clocks. */
import type {
  AssetWorldClient,
  Command,
  EntityRef,
  WorldPersistenceHostClient,
} from "@ipp/client";
import {
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";

type Host = WorldPersistenceHostClient<AssetWorldClient>;
function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

export async function scalingAndInspection(
  host: Host,
  count: number,
  deep: boolean,
) {
  const client = await host.createWorld({ temporary: true });
  const entities: bigint[] = [];
  const started = performance.now();
  for (let start = 0; start < count; start += 256) {
    const operations: Command[] = [];
    const size = Math.min(256, count - start);
    for (let offset = 0; offset < size; offset++) {
      const entity: EntityRef = { kind: "alias", alias: offset + 1 };
      const parent: EntityRef =
        offset === 0
          ? { kind: "handle", id: entities.at(-1) ?? 0n }
          : { kind: "alias", alias: offset };
      operations.push(
        createEntity(offset + 1, `scaling-${start + offset}`),
        insertComponent(client, "Transform", entity, {
          x: deep ? 0.001 : start + offset,
        }),
      );
      if (deep && start + offset > 0)
        operations.push({
          kind: "insertComponent",
          entity,
          component: client.components.Hierarchy!.id,
          fields: [
            {
              offset: client.components.Hierarchy!.fields.parent!.offset,
              value: { kind: "entity", value: parent },
            },
          ],
        });
    }
    const outcome = successfulBatch(await client.batch(operations));
    entities.push(...outcome.aliases.map((entry) => entry.id));
  }
  const created = performance.now();
  let cursor = 0n;
  let previousTick = 0n;
  let observed = 0;
  do {
    const page = await client.inspectPage({
      collection: "entities",
      after: cursor,
    });
    check(
      page.entities.length <= 256 && page.entities.length > 0,
      "bounded nonempty page",
    );
    check(page.tick >= previousTick, "page ticks remain monotonic");
    for (const entity of page.entities) {
      check(entity.id > cursor, "ordered exclusive cursor");
      cursor = entity.id;
      observed++;
    }
    previousTick = page.tick;
    if (page.next === 0n) break;
    check(
      page.next === cursor,
      "continuation resumes after the last returned record",
    );
  } while (observed <= count);
  check(observed === count, "pagination loses or duplicates identities");
  const exact = await client.inspectPage({
    collection: "entities",
    target: entities[count - 1]!,
  });
  check(
    exact.entities.length === 1 &&
      exact.entities[0]!.id === entities[count - 1],
    "targeted read returns exactly one identity",
  );
  const saving = performance.now();
  const bytes = await host.saveWorld();
  const saved = performance.now();
  await client.close();
  const loading = performance.now();
  const restored = await host.loadWorld(bytes, {
    symbolicId: "restored-scaling",
  });
  const loaded = performance.now();
  const all = await restored.inspect();
  check(all.entities.length === count, "bulk restoration loses entities");
  const deleting = performance.now();
  for (let start = 0; start < count; start += 256)
    successfulBatch(
      await restored.batch(
        all.entities.slice(start, start + 256).map((entity) => ({
          kind: "delete",
          entity: { kind: "handle", id: entity.id },
        })),
      ),
    );
  const deleted = performance.now();
  check(
    (await restored.inspectPage({ collection: "entities" })).entities.length ===
      0,
    "mass deletion leaves entities",
  );
  await restored.close();
  return {
    count,
    deep,
    bytes: bytes.length,
    createMs: created - started,
    saveMs: saved - saving,
    loadMs: loaded - loading,
    deleteMs: deleted - deleting,
    lastPageTick: previousTick.toString(),
  };
}
