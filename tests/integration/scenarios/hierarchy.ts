import type { AnimationWorldClient, LifecycleNotification } from "@ipp/client";
import {
  AnimationFixture,
  check,
  type AnimationContract,
  type AnimationRecord,
} from "../animation-fixtures.js";
import { successfulBatch } from "../camera-fixtures.js";

/** Same declarations, deletion/overlay policy and diagnostics over WebSocket and worker. */
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
  let owner: bigint | undefined;
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
      Hierarchy: { parent: a },
      LookAt: { target: b },
    });
    ids.push(child);
    const outcome = successfulBatch(
      await client.batch([
        { kind: "createStateOverlayOwner", alias: 10 },
        {
          kind: "attachEntityOverlayBinding",
          owner: { kind: "alias", alias: 10 },
          alias: 11,
          symbolicId: "hierarchy-life-child",
          mode: "bound",
        },
        {
          kind: "attachComponentStateOverlay",
          owner: { kind: "alias", alias: 10 },
          binding: { kind: "alias", alias: 11 },
          alias: 12,
          component: client.components.Hierarchy!.id,
          mode: "auto",
          fields: fixture.fields("Hierarchy", { parent: b }),
        },
      ]),
    );
    owner = outcome.stateOverlays.find((v) => v.alias === 10)!.id;
    successfulBatch(
      await client.batch([
        { kind: "delete", entity: { kind: "handle", id: a } },
      ]),
    );
    ids.splice(ids.indexOf(a), 1);
    const values = async () => {
      const state = await fixture.inspect();
      const entity = state.entities.find((v) => v.id === child)!;
      const hierarchy = client.components.Hierarchy!.id;
      return {
        base: entity.base.find((v) => v.component === hierarchy)!.fields.parent,
        effective: entity.effective.find((v) => v.component === hierarchy)!
          .fields.parent,
      };
    };
    let state = await values();
    check(
      state.base === 0n && state.effective === b,
      "deleted hidden producer parent must clear without losing the overlay",
    );
    successfulBatch(
      await client.batch([
        {
          kind: "releaseStateOverlayOwner",
          owner: { kind: "handle", id: owner },
        },
      ]),
    );
    owner = undefined;
    state = await values();
    check(
      state.effective === 0n,
      "release must reveal the cleared root relationship",
    );
    successfulBatch(
      await client.batch(fixture.set(child, "Hierarchy", { parent: b })),
    );
    const cyclic = await client.batch([
      {
        kind: "insertComponent",
        entity: { kind: "handle", id: b },
        component: client.components.Hierarchy!.id,
        fields: fixture.fields("Hierarchy", { parent: child }),
      },
    ]);
    check(!cyclic.ok, "hierarchy cycle was accepted");
    successfulBatch(
      await client.batch([
        {
          kind: "removeComponent",
          entity: { kind: "handle", id: b },
          component: client.components.Hierarchy!.id,
        },
      ]),
    );
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
    state = await values();
    check(
      state.base === 0n && state.effective === 0n,
      "deleted parent rebound to replacement",
    );
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
      observed.has(client.components.Hierarchy!.id) &&
        observed.has(client.components.LookAt!.id),
      "new components are missing lifecycle publication",
    );
    await record("hierarchy.lifecycle", { events, cyclic });
    return { published: true, overlayCleanup: true, cycleRejected: true };
  } finally {
    if (owner !== undefined)
      await client.batch([
        {
          kind: "releaseStateOverlayOwner",
          owner: { kind: "handle", id: owner },
        },
      ]);
    await subscription.unsubscribe();
    await client.batch(
      ids.map((id) => ({ kind: "delete", entity: { kind: "handle", id } })),
    );
  }
}
