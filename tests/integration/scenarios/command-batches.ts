import { clientAssetSource } from "../../../packages/ipp-client/src/asset-sources.js";
import { applyCommandPages } from "../../../packages/ipp-client/src/command-pages.js";
import type { Command } from "@ipp/client";
import type { Client, AnimationWorldClient, Request } from "@ipp/client";
import {
  AnimationFixture,
  check,
  type AnimationContract,
  type AnimationRecord,
} from "../animation-fixtures.js";
import { createEntity } from "../camera-fixtures.js";
import { settledAsset } from "../asset-fixtures.js";

/** Source delivery exceeds command limits and progresses while the World is held. */
export async function assetSourceDuringBatch(
  client: AnimationWorldClient,
  contract: AnimationContract,
  record: AnimationRecord,
) {
  const count = 90_000;
  const bytes = contract.encodeAnimationClip({
    duration: count,
    tracks: [
      {
        property: {
          component: client.components.Scalar!.id,
          offsets: [client.components.Scalar!.fields.value!.offset],
        },
        keys: Array.from({ length: count }, (_, time) => ({
          time,
          value: { kind: "f32" as const, value: 9 },
        })),
      },
    ],
  });
  check(
    bytes.length > 1_048_576,
    "fixture must exceed the old command byte limit",
  );
  const source = clientAssetSource(client.session, 10, "large-during-batch");
  const id = await client.beginBatch();
  const first = await client.batchChunk(id, [
    createEntity(1, "provider-during-batch"),
  ]);
  check(first.ok, "initial chunk failed");
  const delivery = client.registerAsset(source, bytes.buffer);
  bytes.fill(0); // The provider owns a snapshot; the caller may reuse its input.
  await delivery;
  const next = await client.batchChunk(id, []);
  check(
    next.ok && next.tick === first.tick,
    "source delivery evaluated or expired the held World",
  );
  await client.endBatch(id);
  const loaded = await settledAsset(client, source);
  check(loaded.status === "loaded", `source decode failed: ${loaded.error}`);
  await record("source-during-batch", {
    source,
    bytes: bytes.length,
    heldTick: next.tick,
    loaded,
  });
  let duplicate = false;
  try {
    await client.registerAsset(source, new ArrayBuffer(0));
  } catch {
    duplicate = true;
  }
  check(duplicate, "immutable source was replaced");
  await client.releaseAsset(source);
}

/** Production transport proof: buffer acknowledgements never imply a frame. */
export async function commandBatches(client: Client, record: AnimationRecord) {
  const id = await client.beginBatch();
  const first = await client.batchChunk(id, [createEntity(1, "stream-first")]);
  check(first.ok, "first buffer failed");
  const queued = client.batch([createEntity(1, "queued-after-stream")]);
  let unrelatedCompleted = false;
  void queued.then(() => {
    unrelatedCompleted = true;
  });
  const inspection = client.inspect();
  let inspectionCompleted = false;
  void inspection.then(() => {
    inspectionCompleted = true;
  });
  const empty = await client.batchChunk(id, []);
  const second = await client.batchChunk(id, [
    {
      kind: "setMetadata",
      entity: { kind: "alias", alias: 1 },
      metadata: { symbolicId: "stream-final", classes: [] },
    },
  ]);
  check(empty.ok && second.ok, "continuation failed");
  check(
    first.tick === empty.tick && first.tick === second.tick,
    "World evaluated between buffers",
  );
  check(
    !unrelatedCompleted && !inspectionCompleted,
    "unrelated work passed an incomplete batch",
  );
  await client.endBatch(id);
  const committed = await queued;
  const snapshot = await inspection;
  check(
    committed.ok && committed.tick > first.tick,
    "queued work did not resume",
  );
  check(
    snapshot.entities.some(
      (entity) => entity.metadata.symbolicId === "stream-final",
    ),
    "cross-buffer alias was lost",
  );
  check(
    snapshot.entities.some(
      (entity) => entity.metadata.symbolicId === "queued-after-stream",
    ),
    "queued command was lost",
  );
  await client.batchChunk(id, []).then(
    () => {
      throw new Error("completed batch reopened");
    },
    () => {},
  );

  const stalled = await client.beginBatch();
  check(stalled !== id, "Host reused a batch identity");
  let stop = () => {};
  const aborted = new Promise<{
    batchId: bigint;
    tick: bigint;
    message: string;
  }>((resolve) => {
    stop = client.onBatchAborted((failure) => {
      if (failure.batchId === stalled) resolve(failure);
    });
  });
  try {
    const partial = await client.batchChunk(stalled, [
      createEntity(1, "retained-after-timeout"),
    ]);
    check(partial.ok, "partial buffer failed");
    const correction = client.batch([
      {
        kind: "setMetadata",
        entity: { kind: "handle", id: partial.aliases[0]!.id },
        metadata: { symbolicId: "corrected-after-timeout", classes: [] },
      },
    ]);
    const failure = await aborted;
    check(
      failure.tick === partial.tick && /deadline/.test(failure.message),
      "timeout evaluated or failed to report",
    );
    check((await correction).ok, "partial effects could not be corrected");
    await client.endBatch(stalled).then(
      () => {
        throw new Error("expired batch accepted terminator");
      },
      () => {},
    );
    const recovered = await client.inspect();
    check(
      recovered.entities.some(
        (entity) => entity.metadata.symbolicId === "corrected-after-timeout",
      ),
      "timeout rolled back effects",
    );
    await record("command-batches", {
      first,
      empty,
      second,
      committed,
      failure,
      recovered,
    });
  } finally {
    stop();
  }
}

/** Track references commit before their immutable asset bytes have been produced. */
export async function commandBatchBeforeTrackAsset(
  client: AnimationWorldClient,
  contract: AnimationContract,
  record: AnimationRecord,
) {
  const fixture = new AnimationFixture(client, contract, record);
  const id = await client.beginBatch();
  const outcome = await client.batchChunk(id, [
    createEntity(1, "before-bake"),
    {
      kind: "insertComponent",
      entity: { kind: "alias", alias: 1 },
      component: client.components.Scalar!.id,
      fields: fixture.fields("Scalar", { value: 0 }),
    },
  ]);
  check(outcome.ok, "entity declaration failed");
  const entity = outcome.aliases[0]!.id;
  await client.endBatch(id);
  const controller = await fixture.controller([
    fixture.driver(
      entity,
      clientAssetSource(client.session, 10, 88441n).source,
      0,
      "Scalar",
      ["value"],
    ),
  ]);
  const before = await fixture.inspect();
  check(
    fixture.value(before, entity, "Scalar", "value") === 0,
    "missing track changed authored value",
  );
  await client.waitForFrame(before.tick);
  // Production begins here, after entity/controller declarations and a completed frame.
  await fixture.upload(fixture.curve("Scalar", "value", 0, 0), 88441n);
  const after = await fixture.seekPaused(controller, 0.4375);
  check(
    fixture.value(after, entity, "Scalar", "value") === 6,
    "late track did not bind without resubmitting declarations",
  );
  await record("batch-before-track-asset", { outcome, before, after });
}

/** The byte limit deliberately creates a short non-final command buffer. */
export async function byteLimitedCommandBuffers(
  client: Client,
  encode: (request: Request) => Uint8Array<ArrayBuffer>,
  record: AnimationRecord,
) {
  const buffers: { commands: number; bytes: number; tick: bigint }[] = [];
  let inFlight = 0;
  let peakInFlight = 0;
  const data = "x".repeat(32_000);
  function* commands(): Generator<Command> {
    for (let alias = 1; alias <= 40; alias++)
      yield {
        kind: "create",
        alias,
        metadata: { symbolicId: `byte-limit-${alias}`, classes: [data] },
      };
  }
  const outcome = await applyCommandPages(
    {
      beginBatch: () => client.beginBatch(),
      endBatch: (id) => client.endBatch(id),
      batchChunk: async (id, operations) => {
        const bytes = encode({
          session: client.session,
          requestId: 1n,
          body: { kind: "batchChunk", batch: { id, operations } },
        }).byteLength;
        peakInFlight = Math.max(peakInFlight, ++inFlight);
        const applied = await client.batchChunk(id, operations);
        inFlight--;
        buffers.push({
          commands: operations.length,
          bytes,
          tick: applied.tick,
        });
        return applied;
      },
    },
    commands(),
    encode,
  );
  check(
    outcome.ok && outcome.aliases.length === 40,
    "byte-limited streaming lost identities",
  );
  check(
    buffers.length > 1 &&
      buffers.every(
        (buffer) => buffer.commands < 256 && buffer.bytes <= 131_072,
      ),
    "fixture did not split at the byte limit",
  );
  check(
    buffers.every((buffer) => buffer.tick === buffers[0]!.tick),
    "short byte-limited buffer completed the batch",
  );
  await client.waitForFrame(outcome.tick);
  check(
    (
      await client.batch(
        outcome.aliases.map(({ id }) => ({
          kind: "delete",
          entity: { kind: "handle", id },
        })),
      )
    ).ok,
    "byte-limit fixture cleanup failed",
  );
  check(
    peakInFlight > 1 && peakInFlight <= 8,
    "command pages did not pipeline with bounded concurrency",
  );
  const automaticBatch = client.batch([
    ...Array.from({ length: 300 }, (_, index) =>
      createEntity(index + 1, `automatic-${index}`),
    ),
    { kind: "delete", entity: { kind: "alias", alias: 1 } },
  ]);
  const automaticInspections = Array.from({ length: 63 }, () =>
    client.inspectPage({ collection: "summary" }),
  );
  const automatic = await automaticBatch;
  const observedAutomatic = await Promise.all(automaticInspections);
  check(
    automatic.ok && automatic.aliases.length === 300,
    "automatic command paging lost aliases across pages",
  );
  check(
    observedAutomatic.every((inspection) => inspection.tick >= automatic.tick),
    "queued inspection pages overtook automatic command paging",
  );
  check(
    (
      await client.batch(
        automatic.aliases.slice(1).map(({ id }) => ({
          kind: "delete",
          entity: { kind: "handle", id },
        })),
      )
    ).ok,
    "automatic page cleanup failed",
  );
  const rejected = await client.batch([
    ...Array.from({ length: 300 }, (_, index) =>
      createEntity(index + 1, `automatic-failure-${index}`),
    ),
    { kind: "delete", entity: { kind: "alias", alias: 9999 } },
    ...Array.from({ length: 300 }, (_, index) =>
      createEntity(index + 301, `unapplied-${index}`),
    ),
  ]);
  check(
    !rejected.ok &&
      rejected.error.operation === 300 &&
      rejected.aliases.length === 300,
    "paged failure lost its global operation or surviving identities",
  );
  check(
    (
      await client.batch(
        rejected.aliases.map(({ id }) => ({
          kind: "delete",
          entity: { kind: "handle", id },
        })),
      )
    ).ok,
    "paged failure did not permit correction",
  );
  await record("byte-limited-command-buffers", {
    peakInFlight,
    buffers,
    aliases: outcome.aliases.length,
  });
}
