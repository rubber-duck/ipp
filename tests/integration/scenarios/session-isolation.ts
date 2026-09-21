import assert from "node:assert/strict";
import { alias } from "../driver.js";
import { aliasEntity, committed, observed } from "../assertions.js";
import type { ScenarioContext } from "../environment.js";

export async function reconnectStartsWithEmptyWorld(
  context: ScenarioContext,
): Promise<void> {
  const creation = committed(
    await context.execute("create first-session entity", { batchId: 40n }, () =>
      context.driver.submit(
        40n,
        [
          {
            kind: "create",
            alias: "first-session",
            symbolicId: "session-only",
          },
          { kind: "insertScalar", entity: alias("first-session"), value: 17 },
        ],
        { signal: context.signal },
      ),
    ),
  );
  const firstEntity = aliasEntity(creation, "first-session");
  assert.equal(
    observed(
      await context.execute("inspect first-session entity", firstEntity, () =>
        context.driver.inspect(firstEntity, { signal: context.signal }),
      ),
    ).scalar?.base,
    17,
  );

  await context.execute("close first session", {}, () =>
    context.driver.close(),
  );
  const fresh = await context.connectFresh();
  assert.equal(
    await context.execute("verify fresh world is empty", "session-only", () =>
      fresh.findBySymbolicId("session-only", { signal: context.signal }),
    ),
    null,
  );

  const freshCreation = committed(
    await context.execute("create in fresh session", { batchId: 40n }, () =>
      fresh.submit(
        40n,
        [{ kind: "create", alias: "fresh", symbolicId: "fresh-session" }],
        { signal: context.signal },
      ),
    ),
  );
  const freshEntity = aliasEntity(freshCreation, "fresh");
  assert.equal(freshEntity.index, firstEntity.index);
  assert.equal(freshEntity.generation, firstEntity.generation);
  assert.equal(freshEntity.bits, firstEntity.bits);
}
