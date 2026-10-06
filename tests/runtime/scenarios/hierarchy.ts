import type {
  AnimationWorldClient,
  Command,
  LifecycleNotification,
} from "@ipp/client";
import {
  AnimationFixture,
  type AnimationContract,
  type AnimationRecord,
} from "../../fixtures/animation.js";
import { check } from "../../harness/page/checks.js";
import { successfulBatch } from "../../fixtures/commands.js";

function place(entity: bigint, parent: bigint | null): Command {
  return {
    kind: "placeEntity",
    entity: { kind: "handle", id: entity },
    placement: {
      parent: parent === null ? null : { kind: "handle", id: parent },
      before: null,
    },
  };
}

/** Same declarations, placement, deletion policy and diagnostics over WebSocket and worker. */
export async function hierarchyLifecycle(
  client: AnimationWorldClient,
  contract: AnimationContract,
  record: AnimationRecord,
) {
  const fixture = new AnimationFixture(client, contract, record);
  const events: LifecycleNotification[] = [];
  const subscription = await client.subscribeLifecycle(
    { assets: false },
    (event) => events.push(event),
  );
  const ids: bigint[] = [];
  try {
    const a = await fixture.create("hierarchy-life-a", {
      Transform: { x: 10 },
    });
    ids.push(a);
    const b = await fixture.create("hierarchy-life-b", {
      Transform: { x: 20 },
    });
    ids.push(b);
    const child = await fixture.create("hierarchy-life-child", {
      Transform: { x: 1 },
      LookAt: { target: b },
    });
    ids.push(child);
    const parentOf = async () => {
      const state = await fixture.inspect();
      const entity = state.entities.find((v) => v.id === child);
      check(entity, "hierarchy child missing from inspection");
      return entity.link.parent;
    };
    // A placement may name its entity by symbolic id.
    successfulBatch(
      await client.batch([
        {
          kind: "placeEntity",
          entity: { kind: "symbol", symbol: "hierarchy-life-child" },
          placement: { parent: { kind: "handle", id: b }, before: null },
        },
      ]),
    );
    check(
      (await parentOf()) === b,
      "symbolic placement did not move the child",
    );
    successfulBatch(await client.batch([place(child, a)]));
    successfulBatch(
      await client.batch([
        { kind: "delete", entity: { kind: "handle", id: a } },
      ]),
    );
    ids.splice(ids.indexOf(a), 1);
    check(
      (await parentOf()) === null,
      "deleting the parent must leave the child a root",
    );
    successfulBatch(await client.batch([place(child, b)]));
    const cyclic = await client.batch([place(b, child)]);
    check(!cyclic.ok, "hierarchy cycle was accepted");
    successfulBatch(await client.batch([place(b, null)]));
    successfulBatch(
      await client.batch([
        { kind: "delete", entity: { kind: "handle", id: b } },
      ]),
    );
    ids.splice(ids.indexOf(b), 1);
    const replacement = await fixture.create("hierarchy-life-replacement", {
      Transform: { x: 99 },
    });
    ids.push(replacement);
    check((await parentOf()) === null, "deleted parent rebound to replacement");
    const latest = await client.inspect();
    await client.waitForFrame(latest.tick);
    const observed = new Set(
      events.flatMap((event) =>
        event.kind === "change" && event.observation.kind === "component"
          ? [event.observation.component]
          : [],
      ),
    );
    check(
      observed.has(client.components.LookAt!.id),
      "new components are missing lifecycle publication",
    );
    await record("hierarchy.lifecycle", { events, cyclic });
    return { published: true, parentCleanup: true, cycleRejected: true };
  } finally {
    await subscription.unsubscribe();
    await client.batch(
      ids.map((id) => ({ kind: "delete", entity: { kind: "handle", id } })),
    );
  }
}
