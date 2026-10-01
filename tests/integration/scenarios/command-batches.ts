import { clientAssetSource } from "../../../packages/ipp-client/src/asset-sources.js";
import { planCommandPages } from "../../../packages/ipp-client/src/command-pages.js";
import type { Command } from "@ipp/client";
import type {
  Client,
  AnimationWorldClient,
  HostClientBase,
  Request,
} from "@ipp/client";
import {
  AnimationFixture,
  check,
  type AnimationContract,
  type AnimationRecord,
} from "../animation-fixtures.js";
import { createEntity, pageCommands, pageLimits } from "../camera-fixtures.js";
import { settledAsset } from "../asset-fixtures.js";
import {
  LIFECYCLE,
  CONSTRAINTS,
  ASSETS,
  selectSystems,
} from "../system-selections.js";

export async function streamedWorldCommands(
  host: HostClientBase<Client>,
  contract: AnimationContract & {
    encodeRequest(request: Request): Uint8Array<ArrayBuffer>;
  },
) {
  const owned = await host.createWorld({
    selectedSystems: selectSystems(ASSETS, CONSTRAINTS, LIFECYCLE),
    symbolicId: "streamed-commands",
  });
  let client: Client | undefined;
  const records: { kind: string; value: unknown }[] = [];
  const record: AnimationRecord = async (kind, value) => {
    records.push({ kind, value });
  };
  try {
    client = await host.openWorld(owned.reference);
    check(
      client.capabilities.animation,
      "streaming asset fixture requires animation",
    );
    await commandBatches(client, record);
    await byteLimitedCommandBuffers(client, contract.encodeRequest, record);
    await assetSourceDuringBatch(
      client as AnimationWorldClient,
      contract,
      record,
    );
    await commandBatchBeforeTrackAsset(
      client as AnimationWorldClient,
      contract,
      record,
    );
    return records;
  } finally {
    try {
      await client?.close();
    } finally {
      await host.destroyWorld(owned.reference);
    }
  }
}

/** Count messages this World session hands to its transport. */
function countSends(client: Client): { count(): number; restore(): void } {
  const transport = (
    client as unknown as {
      transport: { send(bytes: Uint8Array<ArrayBuffer>): void };
    }
  ).transport;
  const send = transport.send;
  let sent = 0;
  transport.send = (bytes) => {
    sent++;
    send.call(transport, bytes);
  };
  return {
    count: () => sent,
    restore: () => {
      transport.send = send;
    },
  };
}

function creates(count: number, prefix: string): Command[] {
  return Array.from({ length: count }, (_, index) =>
    createEntity(index + 1, `${prefix}-${index}`),
  );
}

/** Source delivery exceeds command limits and progresses while a batch is open. */
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
  const before = await client.inspect();
  // A full page leaves at once; the batch stays open until finish().
  const opened = pageCommands(client) + 1;
  const writer = client.openBatch();
  writer.write(creates(opened, "provider-during-batch"));
  const delivery = client.registerAsset(source, bytes.buffer);
  bytes.fill(0); // The provider owns a snapshot; the caller may reuse its input.
  await delivery;
  const loaded = await settledAsset(client, source);
  check(loaded.status === "loaded", `source decode failed: ${loaded.error}`);
  const during = await client.inspect();
  check(
    during.tick > before.tick &&
      during.entities.length === before.entities.length,
    "an open batch held the World or applied a page early",
  );
  const outcome = await writer.finish();
  check(
    outcome.ok && outcome.aliases.length === opened,
    "open batch lost pages",
  );
  await record("source-during-batch", {
    source,
    bytes: bytes.length,
    openTick: during.tick,
    appliedTick: outcome.tick,
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
  check(
    (
      await client.batch(
        outcome.aliases.map(({ id }) => ({
          kind: "delete",
          entity: { kind: "handle", id },
        })),
      )
    ).ok,
    "open-batch fixture cleanup failed",
  );
}

/** Production transport proof: a paged batch is N requests and applies whole at its final page. */
export async function commandBatches(client: Client, record: AnimationRecord) {
  const paged = pageCommands(client) + 44;
  const sends = countSends(client);
  let outcome: Awaited<ReturnType<Client["batch"]>>;
  let pages: number;
  let interleaved: Awaited<ReturnType<Client["inspectPage"]>>;
  try {
    const writer = client.openBatch();
    writer.write(creates(paged, "paged"));
    pages = sends.count();
    check(pages === 1, "the first full page did not leave immediately");
    // Frames keep completing while the batch is open, without any of its pages.
    interleaved = await client.inspectPage({ collection: "entities" });
    writer.write([
      {
        kind: "setMetadata",
        entity: { kind: "alias", alias: 1 },
        metadata: { symbolicId: "paged-final", classes: [] },
      },
    ]);
    outcome = await writer.finish();
    pages = sends.count() - 1;
  } finally {
    sends.restore();
  }
  check(
    outcome.ok && outcome.aliases.length === paged,
    "the paged batch lost identities",
  );
  check(pages === 2, `a two-page batch sent ${pages} batch requests`);
  check(
    !interleaved.entities.some((entity) =>
      entity.metadata.symbolicId?.startsWith("paged-"),
    ),
    "a page applied before its final page",
  );
  check(
    (await client.inspect()).entities.some(
      (entity) => entity.metadata.symbolicId === "paged-final",
    ),
    "a cross-page alias was lost",
  );

  let stop = () => {};
  const stalled = client.openBatch();
  const aborted = new Promise<{
    batchId: bigint;
    tick: bigint;
    message: string;
  }>((resolve) => {
    stop = client.onBatchAborted((failure) => {
      if (failure.batchId === BigInt(stalled.batchId)) resolve(failure);
    });
  });
  let rejection: unknown;
  try {
    stalled.write(creates(pageCommands(client) + 1, "stalled"));
    const failure = await aborted;
    check(/received no page/.test(failure.message), "deadline not reported");
    rejection = await stalled.finish().then(
      () => {
        throw new Error("an expired batch applied at its final page");
      },
      (error: unknown) => error,
    );
    check(
      rejection instanceof Error && /received no page/.test(rejection.message),
      "the final page of an expired batch was not rejected with its failure",
    );
    check(
      !(await client.inspect()).entities.some((entity) =>
        entity.metadata.symbolicId?.startsWith("stalled-"),
      ),
      "an expired batch applied a page",
    );
    await record("command-batches", {
      pages,
      outcome,
      failure,
      rejection: String(rejection),
    });
  } finally {
    stop();
  }
  check(
    (
      await client.batch(
        outcome.aliases.map(({ id }) => ({
          kind: "delete",
          entity: { kind: "handle", id },
        })),
      )
    ).ok,
    "paged batch cleanup failed",
  );
}

/** Track references commit before their immutable asset bytes have been produced. */
export async function commandBatchBeforeTrackAsset(
  client: AnimationWorldClient,
  contract: AnimationContract,
  record: AnimationRecord,
) {
  const fixture = new AnimationFixture(client, contract, record);
  const outcome = await client.batch([
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

/** The byte limit deliberately creates short non-final pages. */
export async function byteLimitedCommandBuffers(
  client: Client,
  encode: (request: Request) => Uint8Array<ArrayBuffer>,
  record: AnimationRecord,
) {
  const data = "x".repeat(32_000);
  const commands = Array.from(
    { length: 40 },
    (_, index): Command => ({
      kind: "create",
      alias: index + 1,
      metadata: { symbolicId: `byte-limit-${index + 1}`, classes: [data] },
    }),
  );
  const sizes = planCommandPages(commands, encode, pageLimits(client)).map(
    (operations) =>
      encode({
        session: client.session,
        requestId: 1n,
        body: { kind: "submitBatch", batchId: 0, last: true, operations },
      }).byteLength,
  );
  const sends = countSends(client);
  let outcome: Awaited<ReturnType<Client["batch"]>>;
  try {
    outcome = await client.batch(commands);
  } finally {
    sends.restore();
  }
  check(
    outcome.ok && outcome.aliases.length === 40,
    "byte-limited paging lost identities",
  );
  check(
    sizes.length > 1 &&
      sends.count() === sizes.length &&
      sizes.every((bytes) => bytes <= pageLimits(client).bytes),
    `byte-limited paging sent ${sends.count()} requests for ${sizes.length} pages`,
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
  const count = pageCommands(client) + 44;
  const automaticBatch = client.batch([
    ...creates(count, "automatic"),
    { kind: "delete", entity: { kind: "alias", alias: 1 } },
  ]);
  const automaticInspections = Array.from({ length: 63 }, () =>
    client.inspectPage({ collection: "summary" }),
  );
  const automatic = await automaticBatch;
  const observedAutomatic = await Promise.all(automaticInspections);
  check(
    automatic.ok && automatic.aliases.length === count,
    "automatic command paging lost aliases across pages",
  );
  check(
    observedAutomatic.every((inspection) => inspection.tick >= automatic.tick),
    "inspections sent after a batch observed it before it applied",
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
    ...creates(count, "automatic-failure"),
    { kind: "delete", entity: { kind: "alias", alias: 999_999 } },
    ...Array.from({ length: count }, (_, index) =>
      createEntity(index + count + 1, `unapplied-${index}`),
    ),
  ]);
  check(
    !rejected.ok &&
      rejected.error.operation === count &&
      rejected.aliases.length === count,
    "paged failure lost its global operation or surviving identities",
  );
  check(
    !(await client.inspect()).entities.some((entity) =>
      entity.metadata.symbolicId?.startsWith("unapplied-"),
    ),
    "commands after an operation failure applied",
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
    pages: sizes,
    requests: sends.count(),
    aliases: outcome.aliases.length,
  });
}
