import type {
  BatchOutcome,
  Client,
  GuiTarget,
  GuiWorldClient,
  GuiObservedEffect,
  WorldPersistenceHostClient,
} from "@ipp/client";
import {
  createEntity,
  insertComponent,
  successfulBatch,
  aliasId,
} from "../camera-fixtures.js";
import type { GuiTransportProbe } from "../gui-local-transport.js";
import { guiAction } from "../gui-actions.js";
import { control, controlTarget, setFields } from "./gui-lifecycle.js";

/** The exact target of control component `name` on `entity`. */
async function targetOf(
  client: GuiWorldClient,
  entity: bigint,
  name: string,
): Promise<GuiTarget> {
  const target = await controlTarget(
    client,
    entity,
    client.components[name]!.id,
  );
  check(target, `Missing ${name} on ${entity}`);
  return target;
}

/** A request answered after every effect the Host queued before it. */
function answered(client: GuiWorldClient, entity: bigint) {
  return client.inspectPage({
    collection: "entities",
    target: entity,
    limit: 1,
  });
}

export async function foreignGuiObservers(
  issuerHost: WorldPersistenceHostClient<Client>,
  congestedHost: WorldPersistenceHostClient<Client>,
  healthyHost: WorldPersistenceHostClient<Client>,
) {
  const owned = await issuerHost.createWorld({
    selectedSystems: ["ipp.canvas", "ipp.gui", "ipp.lifecycle-publisher"],
  });
  const issuer = (await issuerHost.openWorld(
    owned.reference,
  )) as GuiWorldClient;
  const congested = (await congestedHost.openWorld(
    owned.reference,
  )) as GuiWorldClient;
  const healthy = (await healthyHost.openWorld(
    owned.reference,
  )) as GuiWorldClient;
  try {
    const outcome = await issuer.batch([
      createEntity(1, "large-text"),
      insertComponent(issuer, "GuiTextInput", { kind: "alias", alias: 1 }),
    ]);
    const entity = aliasId(outcome, 1);
    const target = await targetOf(issuer, entity, "GuiTextInput");
    const healthyEffects: GuiObservedEffect[] = [];
    const subscription = await healthy.subscribeGuiEffects((effect) =>
      healthyEffects.push(effect),
    );
    const subscriptions = [];
    for (let index = 0; index < 56; index++)
      subscriptions.push(await congested.subscribeGuiEffects(() => {}));
    const text = "x".repeat(65536);
    // The committed text is an ordinary field write; each submission then
    // publishes the 64 KiB text to every subscription of every connection.
    successfulBatch(
      await issuer.batch(setFields(issuer, entity, "GuiTextInput", { text })),
    );
    const submit = async () =>
      applied(await guiAction(issuer, target, { kind: "submit" }));
    const terminal = await submit();
    await congested.closed;
    await Promise.all(subscriptions.map((subscription) => subscription.closed));
    const current = await control(healthy, entity);
    check(
      current.value.kind === "text" && current.value.value === text,
      "Foreign output failure rejected or rolled back healthy mutation",
    );
    check(
      healthyEffects.length === 1 &&
        healthyEffects[0]!.tick === terminal.tick &&
        healthyEffects[0]!.effect.kind === "submitted" &&
        healthyEffects[0]!.effect.text === text,
      "Healthy observer lost immutable applied effect",
    );
    const second = await submit();
    await answered(healthy, entity);
    check(
      Number(healthyEffects.length) === 2 &&
        healthyEffects[1]!.tick === second.tick &&
        healthyEffects[1]!.id.ordinal > healthyEffects[0]!.id.ordinal,
      "Healthy connection stopped after foreign observer failure",
    );
    await subscription.unsubscribe();
    return {
      independentConnections: 3,
      subscriptionsOnCongestedConnection: subscriptions.length,
      committedTextBytes: text.length,
      healthyEffects: healthyEffects.length,
      congestedConnectionClosed: true,
    };
  } finally {
    await Promise.allSettled([
      issuer.close(),
      congested.close(),
      healthy.close(),
    ]);
    await issuerHost.destroyWorld(owned.reference);
  }
}

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

/** The outcome of an applied action's batch; its tick is the effect's tick. */
function applied(outcome: BatchOutcome) {
  check(
    outcome.ok,
    `GUI action refused: ${outcome.ok ? "" : outcome.error.reason}`,
  );
  return outcome;
}

export async function ordinaryGuiObservations(
  host: WorldPersistenceHostClient<Client>,
  probe: GuiTransportProbe,
) {
  const owned = await host.createWorld({
    selectedSystems: ["ipp.canvas", "ipp.gui", "ipp.lifecycle-publisher"],
  });
  const issuer = (await host.openWorld(owned.reference)) as GuiWorldClient;
  const observer = (await host.openWorld(owned.reference)) as GuiWorldClient;
  const peer = (await host.openWorld(owned.reference)) as GuiWorldClient;
  try {
    const batch = successfulBatch(
      await issuer.batch([
        createEntity(1, "old-parent"),
        createEntity(2, "new-parent"),
        createEntity(3, "button"),
        insertComponent(issuer, "GuiButton", { kind: "alias", alias: 3 }),
        {
          kind: "placeEntity",
          entity: { kind: "alias", alias: 3 },
          placement: { parent: { kind: "alias", alias: 1 }, before: null },
        },
      ]),
    );
    const oldParent = aliasId(batch, 1),
      newParent = aliasId(batch, 2),
      entity = aliasId(batch, 3);
    const target = await targetOf(issuer, entity, "GuiButton");
    const press = () => guiAction(issuer, target, { kind: "press" });
    const received: GuiObservedEffect[] = [];
    let acknowledged = false;
    const hold = probe.holdObservations(observer.session);
    const registering = observer.subscribeGuiEffects((effect) => {
      check(
        !acknowledged || effect.id.ordinal > received.at(-1)!.id.ordinal,
        "Duplicate callback ordinal",
      );
      received.push(effect);
    });
    await hold.first;
    const first = applied(await press());
    const second = applied(await press());
    successfulBatch(
      await issuer.batch([
        {
          kind: "placeEntity",
          entity: { kind: "handle", id: entity },
          placement: {
            parent: { kind: "handle", id: newParent },
            before: null,
          },
        },
      ]),
    );
    const current = (await answered(observer, entity)).entities.find(
      (item) => item.id === entity,
    );
    check(
      current?.link.parent === newParent,
      "Reparent did not commit before historical delivery",
    );
    check(Number(received.length) === 0, "Held observation delivered early");
    await hold.waitFor(3);
    check(
      hold.release() === 3,
      "ACK and two committed observations were not retained together",
    );
    check(
      Number(received.length) === 2 && !acknowledged,
      "Same-stack ACK activation lost callbacks",
    );
    const subscription = await registering;
    acknowledged = true;
    check(
      subscription.start.kind === "subscribed" &&
        !("ordinal" in subscription.start),
      "Cut fabricated a watermark",
    );
    check(
      received[0]!.tick === first.tick &&
        received[1]!.tick === second.tick &&
        received[1]!.id.ordinal > received[0]!.id.ordinal,
      "Effects differ from actual commits",
    );
    for (const effect of received) {
      check(effect.source === "semantic", "Foreign issuer provenance rejected");
      check(
        effect.ancestry[0] === oldParent && effect.ancestry.at(-1) === entity,
        "Historical ancestry replaced by current tree",
      );
      check(
        Object.isFrozen(effect) &&
          Object.isFrozen(effect.target) &&
          Object.isFrozen(effect.ancestry),
        "Observation is mutable",
      );
    }
    await answered(observer, entity);
    const terminal = applied(await press());
    await answered(observer, entity);
    check(
      Number(received.length) === 3 && received[2]!.tick === terminal.tick,
      "One action did not produce exactly one independent observation",
    );
    const prefix = probe.holdObservations(observer.session);
    const fourth = applied(await press());
    await prefix.first;
    const ending = subscription.unsubscribe();
    await prefix.waitFor(2);
    check(
      prefix.release() === 2,
      "Unsubscribe did not follow its retained prefix",
    );
    const end = await ending;
    check(
      (await subscription.closed).kind === "unsubscribed" &&
        end.kind === "unsubscribed",
      "Missing unsubscribe cut",
    );
    check(
      received.at(-1)!.tick === fourth.tick && Number(received.length) === 4,
      "Unsubscribe lost prefix",
    );
    await press();
    await answered(observer, entity);
    check(Number(received.length) === 4, "Callback after unsubscribe cut");
    let peerEffects = 0;
    const peerSubscription = await peer.subscribeGuiEffects(() => {
      peerEffects++;
    });
    const closingSubscription = await observer.subscribeGuiEffects(() => {
      void observer.close();
    });
    await press();
    check(
      (await closingSubscription.closed).kind === "closed",
      "Reentrant observer close did not fence callbacks",
    );
    await answered(peer, entity);
    check(peerEffects === 1, "Reentrant close erased another observer");
    await peerSubscription.unsubscribe();
    check(
      (await host.getRootOutputBinding(owned.reference)) === null,
      "Machine observations required presentation",
    );
    return {
      observations: received.length,
      prefixBeforeUnsubscribe: true,
      historicalAncestry: true,
      ackSynchronous: true,
      reentrantPeer: peerEffects,
    };
  } finally {
    await Promise.allSettled([issuer.close(), observer.close(), peer.close()]);
    await host.destroyWorld(owned.reference);
  }
}

export async function uncertainGuiControlErrors(
  host: WorldPersistenceHostClient<Client>,
  probe: GuiTransportProbe,
) {
  const owned = await host.createWorld({
    selectedSystems: ["ipp.canvas", "ipp.gui", "ipp.lifecycle-publisher"],
  });
  const issuer = (await host.openWorld(owned.reference)) as GuiWorldClient;
  const healthy = (await host.openWorld(owned.reference)) as GuiWorldClient;
  const observers: GuiWorldClient[] = [];
  let healthyEffects = 0;
  try {
    const created = successfulBatch(
      await issuer.batch([
        createEntity(1, "control-error-button"),
        insertComponent(issuer, "GuiButton", { kind: "alias", alias: 1 }),
      ]),
    );
    const entity = aliasId(created, 1);
    const target = await targetOf(issuer, entity, "GuiButton");
    const peer = await healthy.subscribeGuiEffects(() => healthyEffects++);
    for (const operation of ["subscribe", "unsubscribe"] as const) {
      for (const code of [3, 9]) {
        const observer = (await host.openWorld(
          owned.reference,
        )) as GuiWorldClient;
        observers.push(observer);
        let callbacks = 0;
        const active = await observer.subscribeGuiEffects(() => callbacks++);
        const second = await observer.subscribeGuiEffects(() => callbacks++);
        const hold = probe.holdObservations(observer.session);
        const pending =
          operation === "subscribe"
            ? observer.subscribeGuiEffects(() => callbacks++)
            : active.unsubscribe();
        const settled = pending.then(
          () => new Error("Control unexpectedly succeeded"),
          (error: unknown) => error,
        );
        await hold.first;
        applied(await guiAction(issuer, target, { kind: "press" }));
        await hold.waitFor(operation === "subscribe" ? 4 : 2);
        hold.release(code);
        check(
          callbacks === 0,
          "Unknown control error allowed a later same-stack callback",
        );
        const error = await settled;
        check(
          error instanceof Error && error.message.includes(`Host ${code}:`),
          "Control lost unknown-delivery reason",
        );
        check(
          (await second.closed).kind === "closed",
          "Existing subscription was not fenced",
        );
        await observer.closed;
        await answered(healthy, entity);
        check(
          healthyEffects === observers.length,
          "Unknown control result erased healthy commit observation",
        );
        await observer
          .subscribeGuiEffects(() => {})
          .then(
            () => {
              throw new Error("Fenced client admitted another subscription");
            },
            () => {},
          );
      }
    }
    await peer.unsubscribe();
    return {
      unknownControlFailures: observers.length,
      sameStackCallbacksAfterFailure: 0,
      healthyEffects,
    };
  } finally {
    await Promise.allSettled([
      issuer.close(),
      healthy.close(),
      ...observers.map((client) => client.close()),
    ]);
    await host.destroyWorld(owned.reference);
  }
}
