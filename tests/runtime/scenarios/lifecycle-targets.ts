import type {
  Client,
  Command,
  LifecycleWatchEvent,
  WorldPersistenceHostClient,
} from "@ipp/client";
import type { LifecycleTransportProbe } from "../drivers/browser-lifecycle-targets.js";
import type { LifecycleDiagnosticSample } from "../../../packages/ipp-client/src/diagnostics.js";
import { lifecycleSubsets } from "./lifecycle-subsets.js";
import { createEntity, successfulBatch } from "../../fixtures/commands.js";
import { check } from "../../harness/page/checks.js";

type Host = WorldPersistenceHostClient<Client>;

function handle(id: bigint) {
  return { kind: "handle" as const, id };
}

function rename(entity: bigint, symbolicId: string): Command {
  return {
    kind: "setMetadata",
    entity: handle(entity),
    metadata: { symbolicId, classes: [] },
  };
}

export async function lifecycleTargets(
  issuerHost: Host,
  observerHost: Host,
  healthyHost: Host,
  probe: LifecycleTransportProbe,
  diagnostics = false,
) {
  const unsupportedWatch = await rejectedUnsupportedWatch(issuerHost);
  const subsets = await lifecycleSubsets(
    issuerHost,
    observerHost,
    healthyHost,
    probe,
  );
  const world = await issuerHost.createWorld({
    selectedSystems: ["ipp.lifecycle-publisher"],
  });
  const issuer = await issuerHost.openWorld(world.reference);
  const observer = await observerHost.openWorld(world.reference);
  const sibling = await observerHost.openWorld(world.reference);
  const healthy = await healthyHost.openWorld(world.reference);
  try {
    if (diagnostics) await rejectedDiagnostic(probe.statistics(observer, 1n));
    const created = successfulBatch(
      await issuer.batch(
        Array.from({ length: 260 }, (_, index) =>
          createEntity(index + 1, `target-${index}`),
        ),
      ),
    );
    check(created.aliases.length === 260, "Paged creation lost aliases");
    const targets = created.aliases.slice(0, 130).map(({ id }) => id);
    const unrelated = created.aliases.slice(130).map(({ id }) => id);
    const observed: LifecycleWatchEvent[] = [];
    const watch = await observer.watchLifecycle(
      targets.map((entity) => ({
        target: { kind: "entity", entity },
        kinds: 6,
      })),
      (event) => observed.push(event),
    );
    check(
      watch.baselines.length === 130 &&
        watch.baselines.every(
          (entry) => entry.lifetime.kind === "entity" && entry.lifetime.live,
        ),
      "Large target baseline incomplete",
    );
    check(
      new Set(watch.baselines.map((entry) => entry.member.generation)).size ===
        130,
      "Target generations collided",
    );
    const before = probe.records(observer.session);
    const endpoint = watch.baselines[0]!.member.output;
    const workBefore = diagnostics
      ? await probe.statistics(observer, endpoint)
      : undefined;
    successfulBatch(
      await issuer.batch(
        unrelated.map((entity) => ({ kind: "delete", entity: handle(entity) })),
      ),
    );
    await observer.inspectPage();
    const after = probe.records(observer.session);
    const workAfter = diagnostics
      ? await probe.statistics(observer, endpoint)
      : undefined;
    if (workBefore && workAfter) unrelatedDelta(workBefore, workAfter, 130n);
    check(
      after.messages === before.messages &&
        after.bytes === before.bytes &&
        observed.length === 0,
      "Unrelated mutations allocated lifecycle wire output",
    );

    const paged = await observer.watchLifecycle(
      Array.from({ length: 2800 }, (_, index) => ({
        target: {
          kind: "entity" as const,
          entity: targets[index % targets.length]!,
        },
        kinds: 4,
      })),
      () => {},
    );
    check(
      paged.baselines.length === 2800 && paged.cuts.length === 2,
      "Membership request/reply byte paging lost identities",
    );
    let largeWork: ReturnType<typeof unrelatedDelta> | undefined;
    if (diagnostics) {
      const additional = successfulBatch(
        await issuer.batch(
          Array.from({ length: 130 }, (_, index) =>
            createEntity(index + 1, `diagnostic-target-${index}`),
          ),
        ),
      );
      const extraWatch = await observer.watchLifecycle(
        additional.aliases.map(({ id }) => ({
          target: { kind: "entity" as const, entity: id },
          kinds: 4,
        })),
        () => {},
      );
      check(
        extraWatch.baselines.length === 130 &&
          extraWatch.baselines.every(
            (baseline) =>
              baseline.lifetime.kind === "entity" && baseline.lifetime.live,
          ),
        "Diagnostic target growth did not establish fresh live targets",
      );
      const beforeLarge = await probe.statistics(observer, endpoint);
      successfulBatch(
        await issuer.batch([createEntity(901, "diagnostic-unrelated")]),
      );
      largeWork = unrelatedDelta(
        beforeLarge,
        await probe.statistics(observer, endpoint),
        1n,
      );
      await extraWatch.remove();
      successfulBatch(
        await issuer.batch(
          additional.aliases.map(({ id }) => ({
            kind: "delete",
            entity: handle(id),
          })),
        ),
      );
      const foreign = await sibling.watchLifecycle(
        [{ target: { kind: "entity", entity: targets[0]! }, kinds: 4 }],
        () => {},
      );
      await rejectedDiagnostic(
        probe.statistics(observer, foreign.baselines[0]!.member.output),
      );
      await foreign.remove();
      check(
        !observer.closure && !sibling.closure,
        "Diagnostic rejection closed healthy sessions",
      );
    }
    const pagedCuts = await paged.remove();
    check(
      pagedCuts.length === 2 && (await paged.closed).kind === "removed",
      "Paged removal did not finish every acknowledged generation",
    );

    const hold = probe.hold(observer.session);
    let ready = false;
    let synchronousActivation = false;
    let duplicateEvents = 0;
    const duplicatePending = observer
      .watchLifecycle(
        [{ target: { kind: "entity", entity: targets[0]! }, kinds: 2 }],
        () => {
          duplicateEvents++;
          if (duplicateEvents === 1) synchronousActivation = !ready;
        },
      )
      .then((value) => {
        ready = true;
        return value;
      });
    await hold.waitFor(1);
    successfulBatch(
      await issuer.batch([rename(targets[0]!, "after-membership-cut")]),
    );
    await hold.waitFor(3);
    check(
      hold.release() === 3 && synchronousActivation && !ready,
      "ACK did not synchronously activate before same-stack event",
    );
    const duplicate = await duplicatePending;
    check(
      Number(observed.length) === 1 && duplicateEvents === 1,
      "Shared target did not retain distinct users",
    );
    const removeHold = probe.hold(observer.session);
    successfulBatch(
      await issuer.batch([rename(targets[0]!, "before-remove-cut")]),
    );
    await removeHold.waitFor(2);
    const removal = duplicate.remove();
    await removeHold.waitFor(3);
    removeHold.release();
    const cuts = await removal;
    check(
      cuts.length === 1 && Number(duplicateEvents) === 2,
      "Unsubscribe cut lost a handed-off prefix event",
    );
    successfulBatch(
      await issuer.batch([rename(targets[0]!, "after-remove-cut")]),
    );
    await observer.inspectPage();
    check(
      Number(duplicateEvents) === 2 && Number(observed.length) === 3,
      "Removed user received future callbacks or removed other user",
    );
    const fresh = await observer.watchLifecycle(
      [{ target: { kind: "entity", entity: targets[0]! }, kinds: 2 }],
      () => {},
    );
    check(
      fresh.baselines[0]!.member.generation >
        duplicate.baselines[0]!.member.generation,
      "Readd reused a generation",
    );
    await fresh.remove();

    let healthyEvents = 0;
    const healthyWatch = await healthy.watchLifecycle(
      [{ target: { kind: "entity", entity: targets[0]! }, kinds: 4 }],
      () => {
        healthyEvents++;
      },
    );
    let siblingEvents = 0;
    const siblingWatch = await sibling.watchLifecycle(
      [{ target: { kind: "entity", entity: targets[0]! }, kinds: 4 }],
      () => {
        siblingEvents++;
      },
    );
    let lateCallbacks = 0;
    const late = await observer.watchLifecycle(
      [{ target: { kind: "entity", entity: targets[0]! }, kinds: 4 }],
      () => {
        lateCallbacks++;
      },
    );
    // 131 relevant deletions for one endpoint in one frame fit the connection's byte budget:
    // every record is delivered in order and tracking stays live.
    const observedBefore = observed.length;
    const removed = successfulBatch(
      await issuer.batch(
        targets.map((entity) => ({ kind: "delete", entity: handle(entity) })),
      ),
    );
    const snapshotAnswered = await observer.inspectPage().then(
      () => true,
      () => false,
    );
    check(snapshotAnswered, "Bulk deletion failed a later snapshot");
    const deletions = observed.slice(observedBefore);
    check(
      deletions.length === targets.length &&
        deletions.every(
          (event, index) =>
            event.kind === "event" &&
            event.observation.kind === "entity" &&
            event.observation.change === "deleted" &&
            event.observation.entity === targets[index] &&
            (index === 0 ||
              event.sequence > (deletions[index - 1] as typeof event).sequence),
        ) &&
        lateCallbacks === 1,
      `Bulk deletion lost or reordered lifecycle records: ${deletions.length} of ${targets.length}, late ${lateCallbacks}`,
    );
    const postBurst = successfulBatch(
      await issuer.batch([createEntity(900, "post-burst")]),
    ).aliases[0]!.id;
    let postBurstEvents = 0;
    const next = await observer.watchLifecycle(
      [{ target: { kind: "entity", entity: postBurst }, kinds: 4 }],
      () => {
        postBurstEvents++;
      },
    );
    successfulBatch(
      await issuer.batch([{ kind: "delete", entity: handle(postBurst) }]),
    );
    await Promise.all([
      observer.inspectPage(),
      healthy.inspectPage(),
      sibling.inspectPage(),
    ]);
    check(
      postBurstEvents === 1 &&
        healthyEvents === 1 &&
        siblingEvents === 1 &&
        !healthy.closure &&
        !issuer.closure &&
        !observer.closure &&
        !sibling.closure,
      "Bulk deletion ended tracking, closed a connection or poisoned peers",
    );
    await Promise.all([
      next.remove(),
      late.remove(),
      watch.remove(),
      siblingWatch.remove(),
    ]);
    const callbacksAtClose = observed.length;
    successfulBatch(await issuer.batch([createEntity(901, "post-removal")]));
    await healthy.inspectPage();
    check(
      observed.length === callbacksAtClose,
      "Removed tracking replayed callbacks",
    );
    await healthyWatch.remove();
    return {
      entities: 260,
      targets: 130,
      unrelated: 130,
      pagedMembers: 2800,
      addPages: paged.cuts.length,
      removePages: pagedCuts.length,
      unrelatedWireMessages: after.messages - before.messages,
      unrelatedWireBytes: after.bytes - before.bytes,
      subsets,
      unsupportedWatch,
      diagnosticWork:
        workBefore && workAfter
          ? {
              targets130Members130: unrelatedDelta(workBefore, workAfter, 130n),
              targets260Members3060: largeWork,
            }
          : null,
      synchronousActivation,
      duplicateEvents,
      healthyEvents,
      defaultConnectionBytes: 8 * 1024 * 1024,
      bulkDeletions: targets.length,
      completedMutation: removed.ok,
      laterSnapshotAnswered: snapshotAnswered,
    };
  } finally {
    await Promise.allSettled([
      issuer.close(),
      observer.close(),
      sibling.close(),
      healthy.close(),
    ]);
    await issuerHost.destroyWorld(world.reference);
  }
}

/** A watch on a World without the lifecycle publisher is refused with its reason. */
async function rejectedUnsupportedWatch(host: Host) {
  const world = await host.createWorld({
    selectedSystems: ["ipp.constraints"],
  });
  const client = await host.openWorld(world.reference);
  try {
    const created = successfulBatch(
      await client.batch([createEntity(1, "unwatched")]),
    );
    const entity = created.aliases[0]!.id;
    const error = await client
      .watchLifecycle([{ target: { kind: "entity", entity }, kinds: 4 }], () =>
        check(false, "A refused watch delivered an observation"),
      )
      .then(
        () => null,
        (error: unknown) => error,
      );
    const cause = error instanceof Error ? error.cause : undefined;
    check(
      error instanceof Error &&
        error.name === "LifecycleWatchStartError" &&
        cause instanceof Error &&
        "code" in cause &&
        cause.code === "IPP_REQUEST_REJECTED" &&
        error.message.includes("does not select ipp.lifecycle-publisher"),
      `A watch without the lifecycle publisher must be refused with its reason: ${String(error)}`,
    );
    successfulBatch(await client.batch([rename(entity, "still-connected")]));
    check(!client.closure, "A refused watch closed its connection");
    return error.message;
  } finally {
    await client.close();
    await host.destroyWorld(world.reference);
  }
}

async function rejectedDiagnostic(pending: Promise<LifecycleDiagnosticSample>) {
  const error = await pending.then(
    () => null,
    (error: unknown) => error,
  );
  check(
    error instanceof Error &&
      "code" in error &&
      error.code === "IPP_REQUEST_REJECTED",
    "Stale or absent diagnostics must reject explicitly",
  );
}

function unrelatedDelta(
  before: LifecycleDiagnosticSample,
  after: LifecycleDiagnosticSample,
  lookups: bigint,
) {
  check(
    !before.work.saturated &&
      !after.work.saturated &&
      !before.traffic.saturated &&
      !after.traffic.saturated,
    "Saturated counters cannot establish exact deltas",
  );
  check(
    before.world.id === after.world.id &&
      before.world.incarnation === after.world.incarnation &&
      before.output === after.output,
    "Diagnostic lifetime changed across the sample",
  );
  const delta = {
    lookups: after.work.lookups - before.work.lookups,
    recipientVisits: after.work.recipientVisits - before.work.recipientVisits,
    queuedEvents: after.traffic.queuedEvents - before.traffic.queuedEvents,
    queuedBytes: after.traffic.queuedBytes - before.traffic.queuedBytes,
  };
  check(
    delta.lookups === lookups &&
      delta.recipientVisits === 0n &&
      delta.queuedEvents === 0n &&
      delta.queuedBytes === 0n,
    "Unrelated transition did nonconstant index work or retained output",
  );
  return {
    lookups: delta.lookups.toString(),
    recipientVisits: "0",
    queuedEvents: "0",
    queuedBytes: "0",
  };
}
