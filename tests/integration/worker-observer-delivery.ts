import {
  createWorkerHost,
  type WorkerEndpoint,
} from "../../packages/ipp-client/src/worker.js";
import { PortTransport } from "../../packages/ipp-client/src/transport.js";
import type { DeliveryTrace } from "./gui-observer-endpoint.js";
import type {
  Client,
  GuiTarget,
  GuiWorldClient,
  WorldPersistenceHostClient,
} from "@ipp/client";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "./camera-fixtures.js";
import { guiAction } from "./gui-actions.js";

interface Urls {
  generated: string;
  workerScript: string;
  wasm: string;
  origin: string;
}
interface EndpointResult {
  reason: string;
  outputBytes: number;
  outputMessages: number;
  milliseconds: number;
}

export async function workerObserverDelivery(
  urls: Urls,
  crash = false,
  mode: "development" | "production" = "development",
) {
  const contract = await import(urls.generated);
  const fixture = `${urls.origin}/target/multiplex-tests/gui-observer-endpoint${mode === "production" ? ".production" : ""}.js`;
  const probe = crash ? new WorkerDeliveryProbe() : undefined;
  const extraEndpoints: WorkerEndpoint[] = [];
  const owner = createWorkerHost(
    probe?.runtimeUrl(fixture, urls.workerScript) ?? urls.workerScript,
    urls.wasm,
    contract.MAX_MESSAGE_BYTES,
  );
  const issuerHost: WorldPersistenceHostClient<Client> =
    await contract.IppHostClient.connectTransport(owner.connect());
  const healthyHost: WorldPersistenceHostClient<Client> =
    await contract.IppHostClient.connectTransport(owner.connect());
  const endpoint = owner.openPort();
  const consumer = new Worker(fixture, { type: "module" });
  const waiters = new Map<
    string,
    { resolve(value: any): void; reject(error: Error): void }
  >();
  let failure: Error | undefined;
  let resumed = false;
  const wait = (kind: string): Promise<any> => {
    if (failure) return Promise.reject(failure);
    return new Promise((resolve, reject) =>
      waiters.set(kind, { resolve, reject }),
    );
  };
  consumer.onmessage = (event) => {
    const message = event.data;
    if (message.type === "disposed") {
      endpoint.dispose();
      return;
    }
    if (message.type === "resumed") {
      resumed = true;
      return;
    }
    if (message.type === "test-error") {
      failure = new Error(message.message);
      for (const waiter of waiters.values()) waiter.reject(failure);
      waiters.clear();
      return;
    }
    waiters.get(message.type)?.resolve(message);
    waiters.delete(message.type);
  };
  const owned = await issuerHost.createWorld({
    selectedSystems: ["ipp.canvas", "ipp.gui", "ipp.lifecycle-publisher"],
  });
  const issuer = (await issuerHost.openWorld(
    owned.reference,
  )) as GuiWorldClient;
  const healthy = (await healthyHost.openWorld(
    owned.reference,
  )) as GuiWorldClient;
  try {
    const text = "x".repeat(65_536);
    let entity = 0n;
    for (let index = 1; index <= 15; index++) {
      const created = successfulBatch(
        await issuer.batch([
          createEntity(index, `large-${index}`),
          insertComponent(
            issuer,
            "GuiTextInput",
            { kind: "alias", alias: index },
            { text },
          ),
        ]),
      );
      if (index === 1) entity = aliasId(created, index);
    }
    const target = await textInputTarget(issuer, owned.reference, entity);
    const expectedDeliveries: string[] = [];
    // A request answered after every effect the Host queued before it.
    const settled = (client: Client) =>
      client.inspectPage({ collection: "entities", target: entity, limit: 1 });
    // Each semantic submit publishes the 64 KiB committed text to every
    // subscriber; a stalled subscriber's endpoint accumulates it.
    const submit = async () => {
      const outcome = await guiAction(issuer, target, { kind: "submit" });
      if (!outcome.ok)
        throw new Error(`Healthy issuer refused: ${outcome.error.reason}`);
      // The submission is published with the tick of the applying frame.
      expectedDeliveries.push(`${outcome.tick}`);
    };
    let healthyEffects = 0;
    let lastOrdinal = 0n;
    const observedDeliveries: string[] = [];
    const subscription = await healthy.subscribeGuiEffects((effect) => {
      if (effect.id.ordinal <= lastOrdinal)
        throw new Error("Healthy effects not ordered");
      if (
        effect.id.world.id !== owned.reference.id ||
        effect.id.world.incarnation !== owned.reference.incarnation ||
        effect.target.entity !== target.entity ||
        effect.target.component !== target.component ||
        effect.target.incarnation !== target.incarnation ||
        effect.source !== "semantic" ||
        effect.effect.kind !== "submitted" ||
        effect.effect.text !== text
      )
        throw new Error(
          "Healthy observer received a foreign or incorrect effect",
        );
      observedDeliveries.push(`${effect.tick}`);
      lastOrdinal = effect.id.ordinal;
      healthyEffects++;
    });
    const ready = wait("ready");
    consumer.postMessage(
      {
        type: "init",
        generated: urls.generated,
        world: owned.reference,
        connection: endpoint.connection,
        port: endpoint.port,
      },
      [endpoint.port],
    );
    await ready;
    const stalled = wait("stalled");
    const failed = crash
      ? undefined
      : (wait("failed") as Promise<EndpointResult>);
    const started = performance.now();
    consumer.postMessage({ type: "stall", crash });
    await stalled;
    for (let index = 0; index < 48; index++) await submit();
    await settled(healthy);
    if (resumed || healthyEffects !== 48)
      throw new Error(
        "Healthy progress did not finish while physical receiver was blocked",
      );
    let result: EndpointResult;
    let crashEvidence: Record<string, unknown> | undefined;
    let oldDelivery = 0n;
    if (probe) {
      const failed = await probe.wait(
        (record) =>
          record.operation === "failed" &&
          record.connection === endpoint.connection,
      );
      const before = await probe.sample(endpoint.connection);
      if (before.pendingAfter === 0 || before.retainedBytes < 3 * 1024 * 1024)
        throw new Error("No queued physical deliveries before termination");
      const transfers = probe.records.filter(
        (record) =>
          record.operation === "transferred" &&
          record.connection === endpoint.connection,
      );
      const pending = transfers.slice(-before.pendingAfter);
      oldDelivery = pending[0]!.delivery;
      if (
        pending.reduce((sum, record) => sum + record.bytes, 0) !==
        before.retainedBytes
      )
        throw new Error(
          "Transferred payload ledger disagrees with native pending deliveries",
        );
      const closing = probe.records.find(
        (record) =>
          record.operation === "close" &&
          record.connection === endpoint.connection,
      )!;
      if (
        closing.pendingBefore !== before.pendingAfter ||
        closing.pendingAfter !== before.pendingAfter ||
        closing.retainedBytes !== before.retainedBytes
      )
        throw new Error("Failure released undelivered completion credit");
      const killedAt = probe.records.length;
      consumer.terminate();
      await settled(healthy);
      const after = await probe.sample(endpoint.connection);
      if (
        after.pendingAfter !== before.pendingAfter ||
        after.retainedBytes !== before.retainedBytes
      )
        throw new Error(
          "Receiver termination released credit before owner disposal",
        );
      endpoint.dispose();
      const disposed = await probe.wait(
        (record) =>
          record.operation === "dispose" &&
          record.connection === endpoint.connection,
        killedAt,
      );
      if (
        disposed.pendingBefore !== before.pendingAfter ||
        disposed.pendingAfter !== 0 ||
        disposed.retainedBytes !== 0 ||
        disposed.bytes !== before.retainedBytes
      )
        throw new Error(
          "Disposal did not release exactly the outstanding physical deliveries",
        );
      if (
        probe.records
          .slice(killedAt)
          .some(
            (record) =>
              record.operation === "complete" &&
              record.connection === endpoint.connection,
          )
      )
        throw new Error("Terminated receiver acknowledged queued output");
      result = {
        reason: failed.reason!,
        outputBytes: before.retainedBytes,
        outputMessages: before.pendingAfter,
        milliseconds: performance.now() - started,
      };
      crashEvidence = {
        terminatedBeforeDrain: true,
        retainedUntilExplicitDisposal: true,
        pending: pending.map((record) => ({
          delivery: record.delivery.toString(),
          bytes: record.bytes,
        })),
      };
    } else {
      result = await failed!;
    }
    if (!/congestion|capacity/i.test(result.reason))
      throw new Error(`Unexpected stalled endpoint failure: ${result.reason}`);
    if (
      result.outputBytes < 3 * 1024 * 1024 ||
      result.outputBytes > 8 * 1024 * 1024
    )
      throw new Error(
        `Physical queue bytes out of bounded range: ${result.outputBytes}`,
      );
    if (result.outputMessages >= 64)
      throw new Error(
        `Entry window, not byte credit, limited the stalled endpoint: ${JSON.stringify(result)}`,
      );
    await submit().catch((error) => {
      throw new Error(`Peer failed after endpoint disposal: ${error}`);
    });
    await settled(healthy);
    if (Number(healthyEffects) !== 49)
      throw new Error("Healthy observation lost after disposal");
    const replacement = await contract.IppHostClient.connectTransport(
      owner.connect(),
    );
    const replacementClient = await replacement.openWorld(owned.reference);
    await settled(replacementClient);
    await replacement.close();
    if (probe) {
      const raw = owner.openPort();
      extraEndpoints.push(raw);
      let receiving = 0n;
      let callbackDelivery = 0n;
      let receiverClosed = false;
      raw.port.addEventListener("message", (event: MessageEvent) => {
        if (event.data.type === "data") receiving = event.data.delivery;
      });
      const transport = new PortTransport(raw.port, raw.connection, () => {
        receiverClosed = true;
      });
      const reentrant =
        await contract.IppHostClient.connectTransport(transport);
      const reentrantClient = await reentrant.openWorld(owned.reference);
      let closing: Promise<void> | undefined;
      await reentrantClient.subscribeGuiEffects(() => {
        callbackDelivery = receiving;
        const heldAt = performance.now();
        while (performance.now() - heldAt < 250) {}
        closing = transport.close();
      });
      const start = probe.records.length;
      await submit().catch((error) => {
        throw new Error(`Reentrant-close issuer failed: ${error}`);
      });
      const closed = await probe.wait(
        (record) =>
          record.operation === "close" && record.connection === raw.connection,
        start,
      );
      if (
        !closing ||
        closed.pendingBefore === 0 ||
        closed.pendingAfter !== closed.pendingBefore ||
        closed.retainedBytes === 0
      )
        throw new Error(
          "Reentrant close failed to retain its in-callback delivery",
        );
      await closing;
      await reentrant.close();
      const completed = await probe.wait(
        (record) =>
          record.operation === "complete" &&
          record.connection === raw.connection &&
          record.delivery === callbackDelivery,
        start,
      );
      const beforeCompletion = probe.records
        .slice(0, probe.records.indexOf(completed))
        .findLast((record) => record.connection === raw.connection);
      const transferred = probe.records.find(
        (record) =>
          record.operation === "transferred" &&
          record.connection === raw.connection &&
          record.delivery === callbackDelivery,
      );
      if (
        completed.result !== 1 ||
        completed.pendingBefore <= 0 ||
        completed.pendingAfter !== completed.pendingBefore - 1 ||
        !transferred ||
        completed.bytes !== transferred.bytes ||
        !beforeCompletion ||
        beforeCompletion.retainedBytes - completed.retainedBytes !==
          completed.bytes
      )
        throw new Error(
          "Reentrant callback ACK failed to release exactly its pending ticket and payload",
        );
      if (completed.pendingAfter === 0 || completed.retainedBytes === 0)
        throw new Error(
          "Held receiving scheduler did not leave later Host output queued",
        );
      const drained = await probe.sample(raw.connection);
      if (
        !receiverClosed ||
        drained.pendingAfter !== 0 ||
        drained.retainedBytes !== 0
      )
        throw new Error(
          "Reentrant connection did not drain before owner disposal",
        );
      if (
        probe.records.some(
          (record) =>
            record.operation === "dispose" &&
            record.connection === raw.connection,
        )
      )
        throw new Error(
          "Reentrant endpoint was disposed before its drain was verified",
        );
      raw.dispose();
      const disposed = await probe.wait(
        (record) =>
          record.operation === "dispose" &&
          record.connection === raw.connection,
        start,
      );
      if (
        disposed.result !== 1 ||
        disposed.pendingBefore !== 0 ||
        disposed.pendingAfter !== 0 ||
        disposed.bytes !== 0 ||
        disposed.retainedBytes !== 0
      )
        throw new Error(
          "Reentrant endpoint disposal preceded complete connection drain",
        );
      crashEvidence = {
        ...crashEvidence,
        heldReceiverMilliseconds: 250,
        callbackDelivery: callbackDelivery.toString(),
        callbackPendingBefore: completed.pendingBefore,
        callbackPendingAfter: completed.pendingAfter,
        callbackPayloadBytesReleased: completed.bytes,
        laterPayloadLedgerBytes: completed.retainedBytes,
        drainedBeforeOwnerDisposal: true,
      };

      const late = owner.openPort();
      extraEndpoints.push(late);
      let armed = false;
      let injected = false;
      late.port.addEventListener("message", (event: MessageEvent) => {
        if (
          !armed ||
          injected ||
          event.data.type !== "data" ||
          event.data.bytes.byteLength < 65_536
        )
          return;
        injected = true;
        late.port.postMessage({
          type: "ack",
          connection: endpoint.connection,
          delivery: oldDelivery,
        });
      });
      const lateTransport = new PortTransport(late.port, late.connection, () =>
        late.dispose(),
      );
      const lateHost =
        await contract.IppHostClient.connectTransport(lateTransport);
      const lateClient = await lateHost.openWorld(owned.reference);
      const lateStart = probe.records.length;
      armed = true;
      // The inspected 64 KiB text makes the answer large enough to arm the
      // injected ACK.
      await settled(lateClient).catch(() => {});
      const rejected = await probe.wait(
        (record) =>
          record.operation === "failed" &&
          record.connection === late.connection,
        lateStart,
      );
      if (
        !injected ||
        rejected.pendingAfter === 0 ||
        rejected.retainedBytes === 0 ||
        !/Foreign worker connection envelope/.test(rejected.reason!)
      )
        throw new Error("Late old-connection ACK was not fenced");
      const lateClosure = await lateClient.closed;
      if (
        !/Foreign worker connection envelope/.test(lateClosure.reason.message)
      )
        throw new Error("Wrong SDK closure after old-connection ACK");
      await lateHost.close();
      await probe.wait(
        (record) =>
          record.operation === "dispose" &&
          record.connection === late.connection,
        lateStart,
      );
      if (
        probe.records
          .slice(lateStart)
          .some(
            (record) =>
              record.operation === "complete" &&
              record.connection === endpoint.connection,
          )
      )
        throw new Error(
          "Old connection ACK reached native completion after disposal",
        );
      endpoint.dispose();
      const fresh = await contract.IppHostClient.connectTransport(
        owner.connect(),
      );
      const freshClient = await fresh.openWorld(owned.reference);
      await settled(freshClient);
      await fresh.close();
      await settled(healthy);
      if (Number(healthyEffects) !== 50)
        throw new Error("Healthy peer lost progress across close/late ACK");
      if (
        probe.records.some(
          (record) =>
            record.operation === "failed" &&
            record.connection !== endpoint.connection &&
            record.connection !== late.connection,
        )
      )
        throw new Error(
          "Crash or stale ACK failed a healthy physical connection",
        );
      crashEvidence = {
        ...crashEvidence,
        reentrantCloseCompleted: true,
        lateOldConnectionAckRejected: true,
        trace: JSON.parse(probe.evidence()),
      };
    }
    if (expectedDeliveries.join(",") !== observedDeliveries.join(","))
      throw new Error(
        "Healthy observation IDs/ticks differ from issuer terminals",
      );
    await subscription.unsubscribe();
    return {
      ...result,
      physicalConnections: 3,
      stalledSessions: 2,
      healthyEffects,
      progressWhileEndpointBlocked: true,
      replacementConnected: true,
      crash: crashEvidence,
      consumerBundleMode: mode,
      runtimeWorker: urls.workerScript,
    };
  } catch (error) {
    throw new Error(
      `${String(error)}${probe ? `\nPhysical delivery trace: ${probe.evidence()}` : ""}`,
    );
  } finally {
    consumer.terminate();
    endpoint.dispose();
    for (const extra of extraEndpoints) extra.dispose();
    await Promise.allSettled([issuer.close(), healthy.close()]);
    await Promise.allSettled([issuerHost.destroyWorld(owned.reference)]);
    await Promise.allSettled([issuerHost.close(), healthyHost.close()]);
    try {
      await owner.close();
    } finally {
      probe?.close();
    }
  }
}

/** The exact target of the TextInput on `entity`, from a lifecycle baseline. */
async function textInputTarget(
  client: GuiWorldClient,
  world: GuiTarget["world"],
  entity: bigint,
): Promise<GuiTarget> {
  const component = client.components.GuiTextInput!.id;
  const watch = await client.watchLifecycle(
    [{ target: { kind: "component", entity, component }, kinds: 8 }],
    () => {},
  );
  const lifetime = watch.baselines[0]?.lifetime;
  await watch.remove();
  if (lifetime?.kind !== "component" || lifetime.incarnation === null)
    throw new Error("Missing test control");
  return { world, entity, component, incarnation: lifetime.incarnation };
}

class WorkerDeliveryProbe {
  readonly records: DeliveryTrace[] = [];
  readonly channel = new BroadcastChannel(`delivery-${crypto.randomUUID()}`);
  private readonly waiters = new Set<() => void>();
  private overflow = false;

  constructor() {
    this.channel.onmessage = (event: MessageEvent<DeliveryTrace>) => {
      if (this.records.length === 8192) this.overflow = true;
      else this.records.push(event.data);
      for (const notify of this.waiters) notify();
    };
  }

  runtimeUrl(fixture: string, workerScript: string): string {
    const url = new URL(fixture);
    url.searchParams.set("runtime", workerScript);
    url.searchParams.set("trace", this.channel.name);
    return url.href;
  }

  wait(
    predicate: (record: DeliveryTrace) => boolean,
    start = 0,
  ): Promise<DeliveryTrace> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.waiters.delete(check);
        reject(new Error(`Worker trace timeout: ${this.evidence()}`));
      }, 10_000);
      const check = () => {
        const record = this.records.slice(start).find(predicate);
        if (!this.overflow && !record) return;
        clearTimeout(timer);
        this.waiters.delete(check);
        if (this.overflow) reject(new Error("Worker trace capacity exhausted"));
        else resolve(record!);
      };
      this.waiters.add(check);
      check();
    });
  }

  sample(connection: bigint): Promise<DeliveryTrace> {
    const result = this.wait(
      (record) =>
        record.operation === "sample" && record.connection === connection,
      this.records.length,
    );
    this.channel.postMessage({ connection });
    return result;
  }

  evidence(): string {
    return JSON.stringify(this.records, (_, value) =>
      typeof value === "bigint" ? value.toString() : value,
    );
  }

  close(): void {
    this.channel.close();
  }
}
