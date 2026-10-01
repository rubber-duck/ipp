import type {
  AttachmentEffect,
  BatchOperationEffect,
  Client,
  Command,
  HostClientBase,
} from "@ipp/client";
import {
  createEntity,
  pageCommands,
  successfulBatch,
} from "../camera-fixtures.js";

/** The first World attachment effect of an outcome, skipping adoption reports. */
function firstAttachment(outcome: {
  readonly effects: readonly BatchOperationEffect[];
}): AttachmentEffect {
  const effect = outcome.effects.find(
    (item): item is AttachmentEffect => item.kind !== "adopted",
  );
  check(effect, "Outcome carried no attachment effect");
  return effect;
}

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

async function rejected(action: Promise<unknown>) {
  try {
    await action;
  } catch {
    return;
  }
  throw new Error("Expected receipt rejection");
}

async function retired(client: Client, receipt: bigint) {
  for (let attempt = 0; attempt < 20; attempt++) {
    if ((await client.attachmentRetirement(receipt)) === "retired") return;
    await client.waitForFrame();
  }
  throw new Error("Receipt did not retire");
}

export async function attachmentReceipts(host: HostClientBase<Client>) {
  const parent = await host.createWorld({
    selectedSystems: ["ipp.world-attachment"],
    temporary: true,
  });
  const child = await host.createWorld({
    selectedSystems: [],
    temporary: true,
  });
  const client = await host.openWorld(parent.reference);
  const peer = await host.openWorld(parent.reference);
  try {
    const component = client.components.WorldAttachment!;
    const write = (entity: bigint): Command => ({
      kind: "setField",
      entity: { kind: "handle", id: entity },
      component: component.id,
      field: {
        offset: component.fields.child!.offset,
        value: { kind: "world", value: child.reference },
      },
    });
    const prefix = Array.from({ length: 2200 }, (_, index) =>
      createEntity(index, `receipt-${index}-${"x".repeat(100)}`),
    );
    const outcome = await client.batch([
      ...prefix,
      {
        kind: "insertComponent",
        entity: { kind: "alias", alias: 0 },
        component: component.id,
        fields: [
          {
            offset: component.fields.child!.offset,
            value: { kind: "world", value: child.reference },
          },
        ],
      },
      { kind: "delete", entity: { kind: "handle", id: 0xffffffffffffffffn } },
    ]);
    check(
      !outcome.ok &&
        outcome.error.operation === 2201 &&
        outcome.aliases.length === 2200,
      "Paged failure lost its applied prefix",
    );
    check(
      outcome.effects.length === 1 && outcome.effects[0]!.operation === 2200,
      "Paged receipt lost its original operation index",
    );
    const first = firstAttachment(outcome).receipt;
    check(
      first.parent.id === parent.reference.id &&
        first.parent.incarnation === parent.reference.incarnation &&
        first.child?.id === child.reference.id,
      "Receipt lifetime identities changed",
    );
    check(
      (await client.attachmentRetirement(first.id)) === "pending",
      "Active attachment was reported retired",
    );
    const foreign = await peer.batch([
      createEntity(0, "foreign-prefix"),
      { kind: "detachWorldAttachment", receipt: first.id },
    ]);
    check(
      !foreign.ok &&
        foreign.error.operation === 1 &&
        foreign.aliases.length === 1,
      "Foreign receipt bypassed operation-level admission",
    );
    const replacement = successfulBatch(
      await client.batch([write(first.anchor)]),
    );
    const second = firstAttachment(replacement).receipt;
    check(
      second.id !== first.id &&
        second.revision !== first.revision &&
        second.incarnation === first.incarnation,
      "Same-value write did not replace revision",
    );
    const superseded = successfulBatch(
      await client.batch([
        { kind: "detachWorldAttachment", receipt: first.id },
      ]),
    );
    check(
      superseded.effects[0]?.kind === "superseded",
      "Stale conditional detach erased replacement",
    );
    check(
      (await client.attachmentRetirement(second.id)) === "pending",
      "Replacement edge was detached",
    );
    await retired(client, first.id);
    const detached = successfulBatch(
      await client.batch([
        { kind: "detachWorldAttachment", receipt: second.id },
      ]),
    );
    check(
      detached.effects[0]?.kind === "detached",
      "Exact conditional detach did not report its effect",
    );
    await retired(client, second.id);
    await client.releaseAttachmentReceipt(first.id);
    await client.releaseAttachmentReceipt(second.id);
    await rejected(client.attachmentRetirement(second.id));
    const released = await client.batch([
      createEntity(0, "released-prefix"),
      { kind: "detachWorldAttachment", receipt: second.id },
    ]);
    check(
      !released.ok &&
        released.error.operation === 1 &&
        released.aliases.length === 1,
      "Released receipt discarded partial identities",
    );
    const detachedAnchor = await client.inspectTreePage({
      root: first.anchor,
      limit: 1,
    });
    check(
      detachedAnchor.nodes.length === 1 &&
        outcome.aliases.find((alias) => alias.alias === 0)?.id === first.anchor,
      "Conditional cleanup deleted its non-owned anchor",
    );
    check(
      (await host.listWorlds()).some(
        (world) => world.id === child.reference.id,
      ),
      "Detach destroyed the independently living child",
    );
    function* interruptedCommands(): Generator<Command> {
      yield createEntity(0, "exceptional-page-anchor");
      yield {
        kind: "insertComponent",
        entity: { kind: "alias", alias: 0 },
        component: component.id,
        fields: [
          {
            offset: component.fields.child!.offset,
            value: { kind: "world", value: child.reference },
          },
        ],
      };
      for (let alias = 1; alias <= pageCommands(client) + 2; alias++)
        yield createEntity(alias, `exceptional-page-${alias}`);
      throw new Error("producer failed after its submitted attachment page");
    }
    // A producer failing after a submitted page leaves nothing applied: the
    // Host only applies a batch at its final page, which never leaves.
    const interrupted = client.openBatch();
    let producerFailed = false;
    try {
      interrupted.write(interruptedCommands());
    } catch (error) {
      producerFailed =
        error instanceof Error && /producer failed/.test(error.message);
    }
    check(producerFailed, "Exceptional producer failure was hidden");
    await rejected(interrupted.finish());
    check(
      !(await client.inspect()).entities.some((entity) =>
        entity.metadata.symbolicId?.startsWith("exceptional-page-"),
      ),
      "A page of an unfinished batch applied",
    );

    const closingReceipt = firstAttachment(
      successfulBatch(
        await client.batch([
          {
            kind: "insertComponent",
            entity: { kind: "handle", id: first.anchor },
            component: component.id,
            fields: [
              {
                offset: component.fields.child!.offset,
                value: { kind: "world", value: child.reference },
              },
            ],
          },
        ]),
      ),
    ).receipt;
    await client.close();
    await rejected(client.attachmentRetirement(closingReceipt.id));
    const replacementClient = await host.openWorld(parent.reference);
    try {
      const stale = await replacementClient.batch([
        createEntity(0, "replacement-session-prefix"),
        { kind: "detachWorldAttachment", receipt: closingReceipt.id },
      ]);
      check(
        !stale.ok && stale.error.operation === 1 && stale.aliases.length === 1,
        "Replacement session reused a closed session receipt",
      );
      const current = firstAttachment(
        successfulBatch(await replacementClient.batch([write(first.anchor)])),
      ).receipt;
      check(
        current.id !== closingReceipt.id,
        "Replacement session recycled receipt identity",
      );
      check(
        (await peer.inspectTreePage({ root: first.anchor, limit: 1 })).nodes
          .length === 1,
        "Session close affected healthy sibling or parent World",
      );
      successfulBatch(
        await replacementClient.batch([
          { kind: "detachWorldAttachment", receipt: current.id },
        ]),
      );
      await retired(replacementClient, current.id);
      await replacementClient.releaseAttachmentReceipt(current.id);
    } finally {
      await replacementClient.close();
    }
    return {
      pagedPrefix: prefix.length,
      receiptOperation: 2200,
      foreignPrefix: true,
      sameValueRevision: true,
      superseded: true,
      retired: true,
      releasedPrefix: true,
      exceptionalPageReceiptCleanup: true,
      closedSessionReplacementReceiptRejected: true,
    };
  } finally {
    await peer.close();
    await client.close();
    await host.destroyWorld(parent.reference);
    await host.destroyWorld(child.reference);
  }
}
