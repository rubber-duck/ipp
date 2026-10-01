import type {
  Client,
  LifecycleMemberId,
  LifecycleTargetEvent,
  WorldPersistenceHostClient,
} from "@ipp/client";
import { isLifecycleWatchRemoveError } from "@ipp/client";
import type { LifecycleTransportProbe } from "../lifecycle-target-transport.js";
import { createEntity, successfulBatch } from "../camera-fixtures.js";

type Host = WorldPersistenceHostClient<Client>;

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

async function failed(pending: Promise<unknown>): Promise<unknown> {
  const result = await pending.then(
    () => null,
    (error: unknown) => error,
  );
  check(result instanceof Error, "Expected observable removal failure");
  return result;
}

function rename(entity: bigint, symbolicId: string) {
  return {
    kind: "setMetadata" as const,
    entity: { kind: "handle" as const, id: entity },
    metadata: { symbolicId, classes: [] },
  };
}

export async function lifecycleSubsets(
  issuerHost: Host,
  observerHost: Host,
  healthyHost: Host,
  probe: LifecycleTransportProbe,
) {
  const world = await issuerHost.createWorld({
    selectedSystems: ["ipp.lifecycle-publisher"],
  });
  const issuer = await issuerHost.openWorld(world.reference);
  const observer = await observerHost.openWorld(world.reference);
  const healthy = await healthyHost.openWorld(world.reference);
  let destroyed = false;
  try {
    const created = successfulBatch(
      await issuer.batch(
        Array.from({ length: 260 }, (_, index) =>
          createEntity(index + 1, `subset-${index}`),
        ),
      ),
    );
    const entities = created.aliases.map((entry) => entry.id);
    const raw: LifecycleTargetEvent[] = [];
    const visible: LifecycleTargetEvent[] = [];
    const disposed = new Set<bigint>();
    const watch = await observer.watchLifecycle(
      entities.map((entity) => ({
        target: { kind: "entity", entity },
        kinds: 2,
      })),
      (event) => {
        if (event.kind !== "event") return;
        raw.push(event);
        if (!disposed.has(event.member.generation)) visible.push(event);
      },
    );
    check(watch.baselines.length === 260, "Coalesced watch lost members");
    check(
      probe.requests(observer.session).adds === 1,
      "260 targets required per-ref add roundtrips",
    );
    const ids = watch.baselines.map((entry) => entry.member);
    const invalid: LifecycleMemberId = {
      output: ids[0]!.output,
      generation: 0xffffffffffffffffn,
    };
    await failed(watch.removeMembers([...ids, invalid]));
    check(
      probe.requests(observer.session).removes === 0,
      "Invalid tail sent a removal prefix",
    );

    const held = probe.hold(observer.session);
    successfulBatch(
      await issuer.batch([rename(entities[0]!, "before-subset-ACK")]),
    );
    await held.waitFor(1);
    disposed.add(ids[0]!.generation);
    const first = watch.removeMembers([ids[0]!]);
    const duplicate = watch.removeMembers([ids[0]!, ids[0]!]);
    await held.waitFor(2);
    held.release();
    const cut = await first;
    check(
      (await duplicate)[0]?.request === cut[0]?.request,
      "Duplicate removal invented a new cut",
    );
    check(
      raw.length === 1 && visible.length === 0,
      "Disposed consumer did not fence handed-off prefix",
    );
    check(
      probe.requests(observer.session).removes === 1,
      "Duplicate removal retransmitted",
    );
    successfulBatch(
      await issuer.batch([rename(entities[1]!, "remaining-member")]),
    );
    await observer.inspectPage();
    check(
      visible.some((event) => event.member.generation === ids[1]!.generation),
      "Subset erased surviving members",
    );

    const peerEvents: LifecycleTargetEvent[] = [];
    const other = await observer.watchLifecycle(
      [{ target: { kind: "entity", entity: entities[1]! }, kinds: 2 }],
      (event) => {
        if (event.kind === "event") peerEvents.push(event);
      },
    );
    await failed(watch.removeMembers([ids[1]!, other.baselines[0]!.member]));
    check(
      probe.requests(observer.session).removes === 1,
      "Foreign group member caused partial send",
    );
    const overlapping = probe.hold(observer.session);
    const firstSubset = watch.removeMembers([ids[2]!, ids[1]!]);
    const secondSubset = watch.removeMembers([ids[2]!, ids[3]!]);
    await overlapping.waitFor(2);
    overlapping.release();
    check(
      (await firstSubset).length === 1 && (await secondSubset).length === 2,
      "Overlapping subset lost original cuts",
    );
    check(
      probe.requests(observer.session).removes === 3,
      "Overlapping removals did not share exact work",
    );
    successfulBatch(
      await issuer.batch([rename(entities[1]!, "other-user-survives")]),
    );
    await observer.inspectPage();
    check(
      peerEvents.length === 1,
      "Subset removed another user of the same target",
    );
    const readdedEvents: LifecycleTargetEvent[] = [];
    const readded = await observer.watchLifecycle(
      [{ target: { kind: "entity", entity: entities[0]! }, kinds: 2 }],
      (event) => {
        if (event.kind === "event") readdedEvents.push(event);
      },
    );
    check(
      readded.baselines[0]!.member.generation !== ids[0]!.generation,
      "Re-add reused a generation",
    );
    await watch.removeMembers([ids[0]!]);
    await failed(watch.removeMembers([readded.baselines[0]!.member]));
    successfulBatch(
      await issuer.batch([rename(entities[0]!, "new-generation-survives")]),
    );
    await observer.inspectPage();
    check(
      readdedEvents.length === 1,
      "Stale group cleanup removed the re-added generation",
    );
    const allCuts = await watch.remove();
    check(
      allCuts.length === 4 && probe.requests(observer.session).removes === 4,
      "Group disposal did not coalesce only remaining members",
    );
    await watch.remove();
    check(
      probe.requests(observer.session).removes === 4,
      "Repeated group disposal retransmitted",
    );
    await other.remove();
    await readded.remove();

    const paged = await observer.watchLifecycle(
      Array.from({ length: 2800 }, (_, index) => ({
        target: {
          kind: "entity" as const,
          entity: entities[index % entities.length]!,
        },
        kinds: 4,
      })),
      () => {},
    );
    check(
      paged.cuts.length === 2,
      "Real multi-page registration fixture did not cross the byte bound",
    );
    let destroying: Promise<void> | undefined;
    probe.afterRemovalAck(observer.session, () => {
      destroying = observerHost.destroyWorld(world.reference).then(() => {
        destroyed = true;
      });
      void destroying.catch(() => {});
    });
    const error = await failed(
      paged.removeMembers(paged.baselines.map((entry) => entry.member)),
    );
    check(
      isLifecycleWatchRemoveError(error),
      "Public SDK import did not recognize generated removal failure",
    );
    check(
      error.cuts.length === 1,
      "Runtime page failure lost its confirmed prefix",
    );
    check(
      error.unconfirmedMembers.length > 0 &&
        error.unconfirmedMembers.length < 2800,
      "Runtime failure fabricated complete removal or lost unconfirmed identities",
    );
    check(error.cause instanceof Error, "Runtime failure lost its cause");
    await destroying;
    check(destroyed, "World destruction fixture did not complete");
    check(
      (await paged.closed).kind === "closed",
      "World destruction failed to fence tracking",
    );
    const retryError = await failed(
      paged.removeMembers(error.unconfirmedMembers),
    );
    check(
      isLifecycleWatchRemoveError(retryError),
      "Public SDK import did not recognize generated retry failure",
    );
    check(
      retryError.unconfirmedMembers.length === error.unconfirmedMembers.length,
      "Closed cleanup fabricated confirmation",
    );

    const peerWorld = await healthyHost.createWorld({ selectedSystems: [] });
    const peer = await healthyHost.openWorld(peerWorld.reference);
    try {
      check(
        successfulBatch(await peer.batch([createEntity(1, "healthy-peer")]))
          .aliases.length === 1,
        "Other World did not progress after removal failure",
      );
    } finally {
      await peer.close();
      await healthyHost.destroyWorld(peerWorld.reference);
    }
    return {
      targets: 260,
      coalescedAddRequests: 1,
      singletonRequests: 1,
      subsetAndRemainingRequests: 4,
      duplicateRequests: 0,
      prefixLocallyFenced: true,
      runtimeFailureCuts: error.cuts.length,
      unconfirmedMembers: error.unconfirmedMembers.length,
      publicRemovalErrorRecognition: true,
      healthyPeer: true,
    };
  } finally {
    await Promise.allSettled([
      issuer.close(),
      observer.close(),
      healthy.close(),
    ]);
    if (!destroyed) await issuerHost.destroyWorld(world.reference);
  }
}
