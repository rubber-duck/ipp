import type {
  WorldGraphDescriptor,
  WorldGraphLoadError,
  WorldReference,
} from "@ipp/client";
import type { HostLifecycleParticipant } from "./host-lifecycle.js";
import {
  createEntity,
  pageCommands,
  successfulBatch,
} from "../../fixtures/commands.js";
import { check } from "../../harness/page/checks.js";

async function rejection(promise: Promise<unknown>): Promise<Error> {
  try {
    await promise;
  } catch (error) {
    check(error instanceof Error, "Expected an Error rejection");
    return error;
  }
  throw new Error("Expected rejection");
}

export async function graphTransfers(create: () => HostLifecycleParticipant) {
  const participant = create();
  const host = await participant.connectHost();
  const originals: WorldReference[] = [];
  const retained: WorldReference[] = [];
  const cleanup = async (worlds: readonly WorldReference[]) => {
    for (let offset = 0; offset < worlds.length; offset += 8)
      await Promise.all(
        worlds
          .slice(offset, offset + 8)
          .map((world) => host.destroyWorld(world)),
      );
  };
  try {
    const parentWorld = await host.createWorld({
      symbolicId: "graph-parent",
      selectedSystems: ["ipp.world-attachment"],
      temporary: true,
    });
    originals.push(parentWorld.reference);
    for (let start = 0; start < 69; start += 8) {
      const children = await Promise.all(
        Array.from({ length: Math.min(8, 69 - start) }, (_, index) =>
          host.createWorld({
            symbolicId: `graph-child-${start + index}`,
            selectedSystems: [],
            temporary: true,
          }),
        ),
      );
      originals.push(...children.map((child) => child.reference));
    }
    const parent = await host.openWorld(parentWorld.reference);
    const attachment = parent.components.WorldAttachment!;
    successfulBatch(
      await parent.batch(
        originals.slice(1).flatMap((child, index) => [
          createEntity(index, `anchor-${index}`),
          {
            kind: "insertComponent" as const,
            entity: { kind: "alias" as const, alias: index },
            component: attachment.id,
            fields: [
              {
                offset: attachment.fields.child!.offset,
                value: { kind: "world" as const, value: child },
              },
            ],
          },
        ]),
      ),
    );
    const child = await host.openWorld(originals[1]!);
    // A save between pages captures the member before the whole batch.
    const open = child.openBatch();
    open.write(
      Array.from({ length: pageCommands(child) + 1 }, (_, index) =>
        createEntity(index, `open-graph-child-${index}`),
      ),
    );
    const during = await host.saveWorld(parent.session);
    check(during.byteLength > 0, "Save waited for or failed on an open batch");
    const applied = successfulBatch(await open.finish());
    successfulBatch(
      await child.batch(
        applied.aliases.map(({ id }) => ({
          kind: "delete" as const,
          entity: { kind: "handle" as const, id },
        })),
      ),
    );
    await child.close();
    const bytes = await host.saveWorld(parent.session);
    const input = bytes.slice();
    const inspecting = host.inspectWorldGraph(input);
    input.fill(0);
    const descriptor = await inspecting;
    check(
      descriptor.nodes.length === originals.length,
      "Inspection lost graph metadata pages or did not pin caller bytes",
    );
    check(
      (await host.listWorlds()).length === originals.length,
      "Preview published Worlds",
    );
    const stale = participant.latestTransferJob;
    const names = (suffix: string) => (graph: WorldGraphDescriptor) => {
      check(
        graph.nodes.length === originals.length &&
          graph.root === descriptor.root,
        "Rename callback inspected another transfer",
      );
      return new Map(
        graph.nodes.map((node) => [node.id, `${node.symbolicId}-${suffix}`]),
      );
    };

    const published = participant.holdReplies("worldGraphLoaded");
    const loading = host.loadWorld(bytes, { worldNames: names("copy") });
    await published;
    for (const kind of ["cancel", "read", "ack"] as const) {
      participant.replaceNextHostRequestWithTransfer(kind, stale);
      await rejection(host.listWorlds());
      check(
        (await host.listWorlds()).length === originals.length * 2,
        "A stale transfer request destroyed newer published Worlds",
      );
      check(
        (await parent.inspect()).entities.length === 69,
        "Stale transfer damaged a healthy session",
      );
    }
    participant.releaseReplies();
    const loaded = await loading;
    retained.push(...loaded.created.values());
    check(
      loaded.created.size === originals.length &&
        loaded.created.get(descriptor.root)?.id === loaded.root.id,
      "Created World journal incomplete",
    );
    const copied = await host.openWorld(loaded.root);
    check(
      copied.world!.persistentId === parent.world!.persistentId &&
        copied.world!.id !== parent.world!.id,
      "Graph load lost durable identity or reused runtime identity",
    );
    await copied.close();
    await cleanup(retained.splice(0));

    const aborted = new AbortController();
    const unacknowledged = participant.holdReplies("worldGraphLoaded");
    const cancelling = rejection(
      host.loadWorld(bytes, {
        worldNames: names("cancel"),
        signal: aborted.signal,
      }),
    );
    await unacknowledged;
    aborted.abort();
    participant.releaseReplies();
    await cancelling;
    check(
      (await host.listWorlds()).length === originals.length,
      "Cancelled unacknowledged graph leaked Worlds or cascaded into originals",
    );

    const lateAbort = new AbortController();
    const latePublished = participant.holdReplies("worldGraphLoaded");
    const lateLoading = rejection(
      host.loadWorld(bytes, {
        worldNames: names("late"),
        signal: lateAbort.signal,
      }),
    );
    await latePublished;
    participant.releaseReplies();
    const acknowledged = participant.holdReplies("complete");
    await acknowledged;
    lateAbort.abort();
    participant.rejectNextWorldDestroy();
    participant.releaseReplies();
    const late = (await lateLoading) as WorldGraphLoadError;
    check(
      late.name === "WorldGraphLoadError" &&
        late.graph.created.size === originals.length &&
        late.pendingCleanup.size === 1,
      "Post-ack cancellation lost incomplete identities or exceeded request capacity",
    );
    check(
      (await host.listWorlds()).length === originals.length + 1,
      "Post-ack cancellation failed unrelated cleanup or destroyed the refused lifetime",
    );
    await cleanup([...late.pendingCleanup.values()]);
    check(
      (await host.listWorlds()).length === originals.length,
      "Exact cleanup retry leaked the remaining World",
    );
    check(
      (await parent.inspect()).entities.length === 69,
      "Graph cancellation closed a healthy sibling session",
    );
    await parent.close();
    await cleanup(originals.splice(0));
    return {
      graphNodes: descriptor.nodes.length,
      staleTransferRequests: 3,
      ownedPreview: true,
      unacknowledgedCancellation: true,
      boundedAcknowledgedCleanup: true,
    };
  } finally {
    participant.releaseReplies();
    await cleanup(retained).catch(() => {});
    await cleanup(originals).catch(() => {});
    await participant.close();
  }
}
