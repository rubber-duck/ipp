import type {
  Client,
  LifecycleNotification,
  LifecyclePublication,
  SpatialWorldClient,
  HostClientBase,
} from "@ipp/client";
import { aliasId, createEntity, successfulBatch } from "../camera-fixtures.js";
import type { DriverConnectOptions } from "../driver.js";

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

function changes(events: LifecycleNotification[]): LifecyclePublication[] {
  return events.flatMap((event) => (event.kind === "change" ? [event] : []));
}

async function barrier(client: Client): Promise<void> {
  const state = await client.inspect();
  await client.waitForFrame(state.tick);
}

/** Shared native/browser scenario: generated clients, production commands and owned events. */
export async function lifecycleSubscriptions(
  client: SpatialWorldClient,
  record: DriverConnectOptions["record"],
) {
  const scalar = client.components.Scalar;
  check(scalar?.fields.value, "Scalar contract required");
  const field = (value: number) => ({
    offset: scalar.fields.value!.offset,
    value: { kind: "f32" as const, value },
  });
  const events: LifecycleNotification[] = [];
  const subscription = await client.subscribeLifecycle(
    { assets: false },
    (event) => events.push(event),
  );
  const ids: bigint[] = [];
  try {
    const created = successfulBatch(
      await client.batch([
        createEntity(1, "lifecycle-scalar"),
        {
          kind: "insertComponent",
          entity: { kind: "alias", alias: 1 },
          component: scalar.id,
          fields: [field(1)],
        },
      ]),
    );
    const id = aliasId(created, 1);
    ids.push(id);
    await barrier(client);
    const initial = changes(events).filter(
      (event) =>
        event.observation.kind !== "asset" && event.observation.entity === id,
    );
    check(
      initial.length === 2,
      "Create/insert must publish exactly their applied transitions",
    );
    check(
      initial[0]!.observation.kind === "entity" &&
        initial[0]!.observation.change === "created",
      "Entity creation precedes component insertion",
    );
    const inserted = initial[1]!.observation;
    check(
      inserted.kind === "component" &&
        inserted.change === "inserted" &&
        inserted.previousIncarnation === null &&
        inserted.incarnation !== null,
      "Insertion identifies its fresh incarnation",
    );
    const originalIncarnation = inserted.incarnation;
    const beforeNoop = events.length;
    successfulBatch(
      await client.batch([
        {
          kind: "setField",
          entity: { kind: "handle", id },
          component: scalar.id,
          field: field(1),
        },
        {
          kind: "setMetadata",
          entity: { kind: "handle", id },
          metadata: { symbolicId: "lifecycle-scalar", classes: [] },
        },
      ]),
    );
    await barrier(client);
    check(
      events.length === beforeNoop,
      "Unchanged writes and autonomous frames must publish nothing",
    );

    const failed = await client.batch([
      {
        kind: "setField",
        entity: { kind: "handle", id },
        component: scalar.id,
        field: field(2),
      },
      {
        kind: "delete",
        entity: { kind: "handle", id: 0xffff_ffff_ffff_ffffn },
      },
    ]);
    check(
      !failed.ok && failed.error.operation === 1,
      "Fixture must retain a write before failed deletion",
    );
    await barrier(client);
    const updated = changes(events).at(-1)!.observation;
    check(
      updated.kind === "component" &&
        updated.change === "updated" &&
        updated.incarnation === originalIncarnation,
      "Partial failure preserves the committed change event",
    );

    const filtered: LifecycleNotification[] = [];
    const exact = await client.subscribeLifecycle(
      { entities: false, assets: false, entity: id, component: scalar.id },
      (event) => filtered.push(event),
    );
    successfulBatch(
      await client.batch([
        {
          kind: "insertComponent",
          entity: { kind: "handle", id },
          component: scalar.id,
          fields: [field(3)],
        },
        {
          kind: "setMetadata",
          entity: { kind: "handle", id },
          metadata: { symbolicId: "lifecycle-renamed", classes: ["changed"] },
        },
      ]),
    );
    await barrier(client);
    const replacement = changes(filtered);
    check(
      replacement.length === 1,
      "Exact component filters omit entity metadata",
    );
    const replaced = replacement[0]!.observation;
    check(
      replaced.kind === "component" &&
        replaced.change === "replaced" &&
        replaced.previousIncarnation === originalIncarnation &&
        replaced.incarnation !== originalIncarnation,
      "Replacement identifies old and new incarnations",
    );
    await exact.unsubscribe();
    successfulBatch(
      await client.batch([
        {
          kind: "removeComponent",
          entity: { kind: "handle", id },
          component: scalar.id,
        },
      ]),
    );
    await barrier(client);
    check(filtered.length === 1, "Unsubscription releases future observations");
    const removed = changes(events).at(-1)!.observation;
    check(
      removed.kind === "component" &&
        removed.change === "removed" &&
        removed.incarnation === null,
      "Removal withdraws effective incarnation",
    );
    successfulBatch(
      await client.batch([{ kind: "delete", entity: { kind: "handle", id } }]),
    );
    ids.pop();
    await barrier(client);
    check(
      changes(events).at(-1)!.observation.change === "deleted",
      "Entity deletion publishes its dead identity",
    );
    const sequences = changes(events).map((event) => event.sequence);
    check(
      sequences.every(
        (value, index) => index === 0 || value > sequences[index - 1]!,
      ),
      "Observations preserve World mutation order",
    );
    check(
      events.every(
        (event) => event.session === client.session && event.requestId === 0n,
      ),
      "Event delivery is session scoped and uncorrelated",
    );
    await subscription.unsubscribe();
    const beforeRelease = events.length;
    const final = successfulBatch(
      await client.batch([createEntity(2, "after-lifecycle-release")]),
    );
    ids.push(aliasId(final, 2));
    await barrier(client);
    check(
      events.length === beforeRelease,
      "Released subscription receives no new entities",
    );
    await record("lifecycle.subscriptions", { events, filtered, failed });
    return {
      observations: sequences.length,
      partialFailure: failed.ok === false,
    };
  } finally {
    await subscription.unsubscribe();
    if (ids.length)
      successfulBatch(
        await client.batch(
          ids.map((id) => ({ kind: "delete", entity: { kind: "handle", id } })),
        ),
      );
  }
}

/** Real acquisition, retirement and reacquisition use the generic lifecycle subscription. */
export async function assetLifecycleSubscriptions(
  client: SpatialWorldClient,
  record: DriverConnectOptions["record"],
) {
  const mesh = client.components.MeshInstance;
  check(mesh?.fields.source, "MeshInstance contract required");
  const source = "ipp://mesh/cube?width=1.25&height=1&length=1";
  const events: LifecycleNotification[] = [];
  const subscription = await client.subscribeLifecycle(
    { entities: false, components: false, assets: true },
    (event) => events.push(event),
  );
  const ids: bigint[] = [];
  const create = async (name: string, source: string) => {
    const result = successfulBatch(
      await client.batch([
        createEntity(1, name),
        {
          kind: "insertComponent",
          entity: { kind: "alias", alias: 1 },
          component: mesh.id,
          fields: [
            {
              offset: mesh.fields.source!.offset,
              value: { kind: "string", value: source },
            },
          ],
        },
      ]),
    );
    const id = aliasId(result, 1);
    ids.push(id);
    return id;
  };
  const assets = () =>
    changes(events).flatMap((event) =>
      event.observation.kind === "asset" ? [event.observation] : [],
    );
  const awaitAsset = async (
    source: string,
    status: "loaded" | "failed",
    after = 0,
  ) => {
    for (let attempt = 0; attempt < 120; attempt++) {
      await barrier(client);
      const event = assets()
        .slice(after)
        .find(
          (event) =>
            event.resource.source === source &&
            event.resource.status === status,
        );
      if (event) return event.resource;
    }
    throw new Error(`Subscribed resource did not reach ${status}: ${source}`);
  };
  try {
    const first = await create("lifecycle-asset", source);
    const resource = await awaitAsset(source, "loaded");
    const exactEvents: LifecycleNotification[] = [];
    const exact = await client.subscribeLifecycle(
      { entities: false, components: false, assets: true, asset: resource.id },
      (event) => exactEvents.push(event),
    );
    try {
      successfulBatch(
        await client.batch([
          { kind: "delete", entity: { kind: "handle", id: first } },
        ]),
      );
      ids.splice(ids.indexOf(first), 1);
      await barrier(client);
      const retirement = changes(exactEvents).flatMap((event) =>
        event.observation.kind === "asset" ? [event.observation] : [],
      );
      await record("lifecycle.asset-retirement", {
        events,
        exactEvents,
        resource,
      });
      check(
        retirement.some(
          (event) =>
            event.change === "removed" && event.resource.id === resource.id,
        ),
        "Last consumer release publishes resource identity retirement",
      );
      const beforeReload = assets().length;
      await create("lifecycle-reacquired", source);
      const replacement = await awaitAsset(source, "loaded", beforeReload);
      check(
        replacement.id !== resource.id,
        "Reacquisition after retirement uses a fresh generational identity",
      );
      check(
        changes(exactEvents).length === retirement.length,
        "An exact resource filter never retargets reused storage",
      );
      const failedSource = "ipp://mesh/not-a-recipe";
      await create("lifecycle-unavailable", failedSource);
      const failed = await awaitAsset(failedSource, "failed");
      check(
        !!failed.error,
        "Failed acquisition includes its actual provider diagnostic",
      );
      check(
        changes(events).every((event) => event.observation.kind === "asset"),
        "Asset-only subscription excludes ECS changes",
      );
      await record("lifecycle.assets", {
        events,
        exactEvents,
        resource,
        replacement,
        failed,
      });
      return {
        resource: resource.id,
        replacement: replacement.id,
        observations: events.length,
      };
    } finally {
      await exact.unsubscribe();
    }
  } finally {
    await subscription.unsubscribe();
    if (ids.length)
      successfulBatch(
        await client.batch(
          ids.map((id) => ({ kind: "delete", entity: { kind: "handle", id } })),
        ),
      );
  }
}

/** Reattachment reuses a World while fencing subscription identities and queued data. */
export async function lifecycleSessionIsolation(
  host: HostClientBase<SpatialWorldClient>,
) {
  const first = await host.createWorld({ symbolicId: "lifecycle-session" });
  const oldEvents: LifecycleNotification[] = [];
  await first.subscribeLifecycle({ assets: false }, (event) =>
    oldEvents.push(event),
  );
  const firstSession = first.session;
  await host.detachWorld();
  const next = await host.attachWorld("lifecycle-session");
  check(next.session !== firstSession, "Reattachment requires a new session");
  const fresh: LifecycleNotification[] = [];
  const subscription = await next.subscribeLifecycle(
    { assets: false },
    (event) => fresh.push(event),
  );
  try {
    successfulBatch(
      await next.batch([createEntity(1, "fresh-session-entity")]),
    );
    await barrier(next);
    check(
      oldEvents.length === 0,
      "Detached session must not receive replacement-session events",
    );
    check(
      changes(fresh).length === 1,
      "Replacement session starts one fresh subscription",
    );
    await next.close();
    const third = await host.attachWorld("lifecycle-session");
    successfulBatch(await third.batch([createEntity(2, "after-client-close")]));
    await barrier(third);
    check(
      changes(fresh).length === 1,
      "Client close releases subscriptions in surviving Worlds",
    );
    return {
      firstSession,
      nextSession: next.session,
      observations: fresh.length,
    };
  } finally {
    await subscription.unsubscribe();
    await host.destroyWorld("lifecycle-session");
  }
}

/** A burst in one indivisible batch proves bounded publication without transport mocks. */
export async function lifecycleOverflow(
  client: SpatialWorldClient,
  record: DriverConnectOptions["record"],
) {
  const events: LifecycleNotification[] = [];
  await client.subscribeLifecycle(
    { components: false, assets: false },
    (event) => events.push(event),
  );
  const created = successfulBatch(
    await client.batch(
      Array.from({ length: 140 }, (_, alias) =>
        createEntity(alias, `lifecycle-overflow-${alias}`),
      ),
    ),
  );
  const ids = created.aliases.map((alias) => alias.id);
  try {
    await barrier(client);
    check(
      events.length === 1 && events[0]!.kind === "overflow",
      "A bounded queue publishes exactly one terminal overflow",
    );
    check(
      events[0]!.dropped === 129n,
      "Overflow reports queued and triggering observations",
    );
    const resumed: LifecycleNotification[] = [];
    const next = await client.subscribeLifecycle(
      { components: false, assets: false },
      (event) => resumed.push(event),
    );
    const later = successfulBatch(
      await client.batch([createEntity(0, "lifecycle-after-overflow")]),
    );
    ids.push(aliasId(later, 0));
    await barrier(client);
    check(
      changes(resumed).length === 1,
      "The live session can subscribe after overflow",
    );
    check(events.length === 1, "Overflow ends prior subscriptions permanently");
    await next.unsubscribe();
    await record("lifecycle.overflow", {
      events,
      resumed,
      entities: ids.length,
    });
    return { overflow: events[0], resumed: resumed.length };
  } finally {
    successfulBatch(
      await client.batch(
        ids.map((id) => ({ kind: "delete", entity: { kind: "handle", id } })),
      ),
    );
  }
}

/** Multiple physical clients share one Host, preserving World and session boundaries. */
export async function sharedLifecycleSubscriptions(
  connect: () => Promise<HostClientBase<SpatialWorldClient>>,
  record: DriverConnectOptions["record"],
) {
  const hosts = await Promise.all([connect(), connect(), connect()]);
  const [leftHost, rightHost, peerHost] = hosts;
  check(leftHost && rightHost && peerHost, "Three Host connections required");
  try {
    const left = await leftHost.createWorld({ symbolicId: "lifecycle-left" });
    const right = await rightHost.createWorld({
      symbolicId: "lifecycle-right",
    });
    const peer = await peerHost.attachWorld("lifecycle-left");
    const leftEvents: LifecycleNotification[] = [];
    const rightEvents: LifecycleNotification[] = [];
    const peerEvents: LifecycleNotification[] = [];
    const leftSubscription = await left.subscribeLifecycle(
      { assets: false },
      (event) => leftEvents.push(event),
    );
    const rightSubscription = await right.subscribeLifecycle(
      { assets: false },
      (event) => rightEvents.push(event),
    );
    const peerSubscription = await peer.subscribeLifecycle(
      { assets: false },
      (event) => peerEvents.push(event),
    );
    check(
      leftSubscription.id === rightSubscription.id &&
        leftSubscription.id === peerSubscription.id,
      "Fixture must collide subscription identities across sessions",
    );
    const created = successfulBatch(
      await left.batch([createEntity(1, "left-observed")]),
    );
    const id = aliasId(created, 1);
    await Promise.all([barrier(left), barrier(right), barrier(peer)]);
    check(
      changes(leftEvents).length === 1 && changes(peerEvents).length === 1,
      "Subscribers attached to one World observe its committed creation",
    );
    check(
      rightEvents.length === 0,
      "A subscription in another World observes no creation",
    );
    check(
      changes(leftEvents)[0]!.sequence === changes(peerEvents)[0]!.sequence,
      "Shared World sessions observe the same lifecycle sequence",
    );
    check(
      peerEvents[0]!.session === peer.session &&
        leftEvents[0]!.session === left.session,
      "Transport wraps each observation in its own session fence",
    );
    await leftHost.close();
    successfulBatch(
      await peer.batch([
        {
          kind: "setMetadata",
          entity: { kind: "handle", id },
          metadata: { symbolicId: "after-peer-disconnect", classes: [] },
        },
      ]),
    );
    successfulBatch(await right.batch([createEntity(1, "right-observed")]));
    await Promise.all([barrier(right), barrier(peer)]);
    check(
      leftEvents.length === 1,
      "Disconnect releases the old session's publication",
    );
    check(
      changes(peerEvents).length === 2 && changes(rightEvents).length === 1,
      "A peer disconnect preserves surviving subscriptions and other Worlds",
    );
    await record("lifecycle.shared-sessions", {
      leftEvents,
      rightEvents,
      peerEvents,
    });
    await Promise.all([
      rightSubscription.unsubscribe(),
      peerSubscription.unsubscribe(),
    ]);
    await rightHost.destroyWorld("lifecycle-right");
    await peerHost.destroyWorld("lifecycle-left");
    return {
      left: leftEvents.length,
      peer: peerEvents.length,
      right: rightEvents.length,
    };
  } finally {
    await Promise.all(hosts.map((host) => host.close()));
  }
}
