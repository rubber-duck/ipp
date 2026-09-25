/** GUI root and node lifecycle through a generated client, independent of process launch and wire layout. */
import type {
  AssetWorldClient,
  GuiInspectResponse,
  GuiTextFocusState,
  GuiWorldClient,
  SurfaceWorldClient,
  WorldPersistenceHostClient,
} from "@ipp/client";
import {
  aliasId,
  cameraClient,
  componentFields,
  createEntity,
  insertComponent,
  ORTHOGRAPHIC_CAMERA,
  successfulBatch,
} from "../camera-fixtures.js";

export type GuiTestClient = GuiWorldClient &
  SurfaceWorldClient &
  AssetWorldClient;

/** Property addressing exported by the generated contract under test. */
export interface GuiContractNames {
  /** Generated GuiRoot row helpers: node style and tree properties by offset. */
  GuiRoot: {
    node_styleOffset(slot: number, property: "opacity"): number;
    node_treeOffset(slot: number, property: "parent"): number;
  };
  /** Generated child order of decoded `node_tree` rows. */
  guiTreeChildren(
    rows: ReadonlyMap<
      number,
      { readonly parent: number; readonly order: number }
    >,
  ): Map<number, number[]>;
}

/** One decoded `GuiRoot.node_tree` row as inspection reports it. */
interface NodeTreeRow {
  readonly parent: number;
  readonly order: number;
  readonly kind: number;
}

/** One decoded `GuiRoot.node_style` row as inspection reports it. */
type NodeStyleRow = Readonly<Record<string, unknown>>;

function expect(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

async function rejects(operation: Promise<unknown>, message: string) {
  try {
    await operation;
  } catch {
    return;
  }
  throw new Error(message);
}

function ids(response: GuiInspectResponse) {
  return response.nodes.map((node) => node.id).join();
}

async function guiProperties(client: GuiTestClient, entity: bigint) {
  const snapshot = (await client.inspect()).entities.find(
    (item) => item.id === entity,
  );
  expect(snapshot, "GUI entity disappeared");
  const component = snapshot.effective.find(
    (item) => item.component === client.components.GuiRoot!.id,
  );
  expect(component, "GuiRoot disappeared");
  return { fields: component.fields, properties: component.properties ?? {} };
}

/** Decoded node style rows of one GuiRoot, keyed by node identity. */
async function guiStyleRows(
  client: GuiTestClient,
  entity: bigint,
): Promise<ReadonlyMap<number, NodeStyleRow>> {
  const table = (await guiProperties(client, entity)).fields.node_style as
    | { rows: ReadonlyMap<number, NodeStyleRow> }
    | undefined;
  expect(table?.rows instanceof Map, "GuiRoot inspection omitted node_style");
  return table.rows;
}

/**
 * Exercise root identity, pipelined edits, revision-gated values, ownership
 * rejection, property invalidation and persistence against a live World.
 */
export async function exerciseGuiLifecycle(
  host: WorldPersistenceHostClient<GuiTestClient>,
  { GuiRoot, guiTreeChildren }: GuiContractNames,
) {
  const client = await host.createWorld({ symbolicId: "gui-lifecycle" });
  const batchRef = { kind: "alias", alias: 80 } as const;
  const batchEntity = aliasId(
    await client.batch([
      createEntity(80, "gui-batch-panel"),
      insertComponent(client, "Surface", batchRef, { width: 4, height: 3 }),
      insertComponent(client, "GuiRoot", batchRef),
    ]),
    80,
  );
  const batchRoot = await client.inspectGui({ entity: batchEntity });
  const batchEdits = Array.from({ length: 50 }, (_, index) =>
    index === 0
      ? {
          action: "insert" as const,
          entity: batchEntity,
          rootIncarnation: batchRoot.rootIncarnation,
          id: 1,
          index: 0,
          data: {
            kind: "container" as const,
            containerKind: "column" as const,
          },
        }
      : {
          action: "insert" as const,
          entity: batchEntity,
          rootIncarnation: batchRoot.rootIncarnation,
          id: index + 1,
          parent: 1,
          index: index - 1,
          data: { kind: "text" as const, text: `node ${index}` },
        },
  );
  const batchOutcome = await client.editGuiBatch(batchEdits);
  expect(batchOutcome.ok, "A valid 50-node GUI batch was rejected");
  expect(
    batchOutcome.applied === 50,
    "The GUI batch acknowledged a short prefix",
  );
  await client.waitForFrame();
  let batchedTree = await client.inspectGui({ entity: batchEntity });
  expect(
    batchedTree.nodes.length === 50,
    "The completed GUI batch frame omitted nodes",
  );
  const batchHandle = (id: number) =>
    client.createGuiNodeHandle(batchEntity, batchRoot.rootIncarnation, id);
  const failedBatch = await client.editGuiBatch([
    {
      action: "update",
      handle: batchHandle(2),
      patch: { data: { kind: "text", text: "prefix applied" } },
    },
    {
      action: "insert",
      entity: batchEntity,
      rootIncarnation: batchRoot.rootIncarnation,
      id: 2,
      parent: 1,
      index: 0,
      data: { kind: "text", text: "must fail" },
    },
    {
      action: "update",
      handle: batchHandle(3),
      patch: { data: { kind: "text", text: "suffix must not apply" } },
    },
  ]);
  expect(!failedBatch.ok, "The invalid middle GUI edit was accepted");
  expect(
    failedBatch.applied === 1,
    "The failed GUI batch reported the wrong prefix",
  );
  batchedTree = await client.inspectGui({ entity: batchEntity });
  const prefixNode = batchedTree.nodes.find((node) => node.id === 2);
  const suffixNode = batchedTree.nodes.find((node) => node.id === 3);
  expect(
    prefixNode?.data.kind === "text" &&
      prefixNode.data.text === "prefix applied",
    "The acknowledged GUI prefix was lost",
  );
  expect(
    suffixNode?.data.kind === "text" && suffixNode.data.text === "node 2",
    "A GUI edit after the failed operation was applied",
  );
  const recoveredBatch = await client.editGuiBatch([
    {
      action: "update",
      handle: batchHandle(3),
      patch: { data: { kind: "text", text: "recovered" } },
    },
  ]);
  expect(
    recoveredBatch.ok && recoveredBatch.applied === 1,
    "A correction after a failed GUI batch did not recover",
  );

  // Repeated large patches keep the final tree small while exceeding one
  // message, so the client pages them as one logical batch. Page counts
  // belong to the protocol client tests; this asserts visibility and order.
  const largeUpdates = (character: string) =>
    Array.from({ length: 18 }, (_, index) => ({
      action: "update" as const,
      handle: batchHandle(2),
      patch: {
        data: {
          kind: "text" as const,
          text: `${character.repeat(60_000)}${index}`,
        },
      },
    }));
  const largePromise = client.editGuiBatch(largeUpdates("x"));
  const queuedDirectPromise = client.editGuiBatch([
    {
      action: "update",
      handle: batchHandle(3),
      patch: { data: { kind: "text", text: "queued after multi-page" } },
    },
  ]);
  const queuedInspectionPromise = client.inspectGui({ entity: batchEntity });
  const largeOutcome = await largePromise;
  expect(
    largeOutcome.ok && largeOutcome.applied === 18,
    "A multi-page GUI edit did not apply every edit",
  );
  const queuedDirect = await queuedDirectPromise;
  expect(
    queuedDirect.ok && queuedDirect.applied === 1,
    "A direct GUI edit queued behind a multi-page edit did not complete",
  );
  batchedTree = await queuedInspectionPromise;
  const largeNode = batchedTree.nodes.find((node) => node.id === 2);
  const queuedNode = batchedTree.nodes.find((node) => node.id === 3);
  expect(
    largeNode?.data.kind === "text" && largeNode.data.text.endsWith("17"),
    "The completed multi-page GUI edit lost its final value",
  );
  expect(
    queuedNode?.data.kind === "text" &&
      queuedNode.data.text === "queued after multi-page",
    "An ordinary request overtook a queued direct GUI edit",
  );

  const failedLargePromise = client.editGuiBatch([
    ...largeUpdates("y"),
    {
      action: "insert" as const,
      entity: batchEntity,
      rootIncarnation: batchRoot.rootIncarnation,
      id: 2,
      parent: 1,
      index: 0,
      data: { kind: "text" as const, text: "must fail" },
    },
    {
      action: "update" as const,
      handle: batchHandle(4),
      patch: {
        data: {
          kind: "text" as const,
          text: "multi-page suffix must not apply",
        },
      },
    },
  ]);
  const recoveredLargePromise = client.editGuiBatch([
    {
      action: "update",
      handle: batchHandle(3),
      patch: {
        data: { kind: "text", text: "multi-page recovered" },
      },
    },
  ]);
  const failedInspectionPromise = client.inspectGui({ entity: batchEntity });
  const failedLargeOutcome = await failedLargePromise;
  expect(
    !failedLargeOutcome.ok && failedLargeOutcome.applied === 18,
    "A failed second GUI page lost its global acknowledged prefix",
  );
  const recoveredLargeOutcome = await recoveredLargePromise;
  expect(
    recoveredLargeOutcome.ok && recoveredLargeOutcome.applied === 1,
    "Queued recovery after a failed multi-page GUI edit did not complete",
  );
  batchedTree = await failedInspectionPromise;
  const failedLargePrefix = batchedTree.nodes.find((node) => node.id === 2);
  const failedLargeRecovery = batchedTree.nodes.find((node) => node.id === 3);
  const failedLargeSuffix = batchedTree.nodes.find((node) => node.id === 4);
  expect(
    failedLargePrefix?.data.kind === "text" &&
      failedLargePrefix.data.text.endsWith("17"),
    "The successful prefix on the failed second GUI page was lost",
  );
  expect(
    failedLargeSuffix?.data.kind === "text" &&
      failedLargeSuffix.data.text === "node 3",
    "A suffix after a failed second GUI page was applied",
  );
  expect(
    failedLargeRecovery?.data.kind === "text" &&
      failedLargeRecovery.data.text === "multi-page recovered",
    "An ordinary request overtook queued recovery after batch failure",
  );
  const reusedGateOutcome = await client.editGuiBatch([
    {
      action: "update",
      handle: batchHandle(3),
      patch: {
        data: { kind: "text", text: "automatic gate reused" },
      },
    },
  ]);
  expect(
    reusedGateOutcome.ok && reusedGateOutcome.applied === 1,
    "The automatic GUI gate could not be reused after queued recovery",
  );

  // Observe the real Host between explicit GUI buffers. The held World's
  // frame and inspection cannot complete, while a separate World progresses.
  const streamId = await client.beginBatch();
  const firstStreamPage = await client.editGuiBatchChunk(streamId, [
    {
      action: "update",
      handle: batchHandle(4),
      patch: { data: { kind: "text", text: "stream page one" } },
    },
  ]);
  expect(firstStreamPage.ok, "The first explicit GUI buffer failed");
  let heldFrameCompleted = false;
  let heldInspectionCompleted = false;
  const heldFrame = client.waitForFrame().then(() => {
    heldFrameCompleted = true;
  });
  const heldInspection = client
    .inspectGui({ entity: batchEntity })
    .then((inspection) => {
      heldInspectionCompleted = true;
      return inspection;
    });
  await new Promise((resolve) => setTimeout(resolve, 50));
  expect(
    !heldFrameCompleted && !heldInspectionCompleted,
    "The held GUI World evaluated or served unrelated work between buffers",
  );
  const secondStreamPage = await client.editGuiBatchChunk(streamId, [
    {
      action: "update",
      handle: batchHandle(5),
      patch: { data: { kind: "text", text: "stream page two" } },
    },
  ]);
  expect(secondStreamPage.ok, "The second explicit GUI buffer failed");
  expect(
    !heldFrameCompleted && !heldInspectionCompleted,
    "The held GUI World advanced before explicit termination",
  );
  await client.endBatch(streamId);
  await heldFrame;
  const streamedTree = await heldInspection;
  const streamedFirst = streamedTree.nodes.find((node) => node.id === 4);
  const streamedSecond = streamedTree.nodes.find((node) => node.id === 5);
  expect(
    streamedFirst?.data.kind === "text" &&
      streamedFirst.data.text === "stream page one" &&
      streamedSecond?.data.kind === "text" &&
      streamedSecond.data.text === "stream page two",
    "Explicit GUI buffers did not become visible together",
  );
  successfulBatch(
    await client.batch([
      {
        kind: "removeComponent",
        entity: { kind: "handle", id: batchEntity },
        component: client.components.GuiRoot!.id,
      },
      { kind: "delete", entity: { kind: "handle", id: batchEntity } },
    ]),
  );
  const ref = { kind: "alias", alias: 1 } as const;
  const entity = aliasId(
    await client.batch([
      createEntity(1, "gui-panel"),
      insertComponent(client, "Transform", ref),
      insertComponent(client, "Surface", ref, { width: 4, height: 3 }),
      insertComponent(client, "GuiRoot", ref),
    ]),
    1,
  );
  const empty = await client.inspectGui({ entity });
  expect(empty.nodes.length === 0, "A new GuiRoot must start empty");
  const rootIncarnation = empty.rootIncarnation;
  const handle = (id: number) =>
    client.createGuiNodeHandle(entity, rootIncarnation, id);

  // Edits and inspection pipelined in one ingress drain observe every edit.
  const [, , , pipelined] = await Promise.all([
    client.editGui({
      action: "insert",
      entity,
      rootIncarnation,
      id: 1,
      index: 0,
      data: { kind: "container", containerKind: "column" },
    }),
    client.editGui({
      action: "insert",
      entity,
      rootIncarnation,
      id: 2,
      parent: 1,
      index: 0,
      data: { kind: "checkbox" },
      values: { checked: false },
      style: { color: [0.2, 0.4, 0.6, 1], fontSize: 0.2 },
    }),
    client.editGui({
      action: "insert",
      entity,
      rootIncarnation,
      id: 3,
      parent: 1,
      index: 1,
      data: { kind: "slider" },
      values: { value: 0.25, min: 0, max: 1, step: 0 },
    }),
    client.inspectGui({ entity }),
  ]);
  expect(ids(pipelined) === "1,2,3", `Pipelined inspection ${ids(pipelined)}`);

  // A styled panel keeps every node's style in one row table: inspection
  // decodes all rows through the generated client, not named properties.
  const denseStyle = {
    enabled: true,
    width: 1,
    height: 1,
    minWidth: 0.25,
    minHeight: 0.25,
    maxWidth: 2,
    maxHeight: 2,
    padding: [0.01, 0.02, 0.03, 0.04],
    margin: [0.04, 0.03, 0.02, 0.01],
    flex: 1,
    alignX: 0,
    alignY: 0,
    color: [0.2, 0.4, 0.6, 1],
    backgroundColor: [0.1, 0.2, 0.3, 1],
    opacity: 0.75,
    fontSize: 0.1,
  } as const;
  for (let id = 4; id <= 19; id++) {
    await client.editGui({
      action: "insert",
      entity,
      rootIncarnation,
      id,
      parent: 1,
      index: id - 2,
      data: { kind: "text", text: `dense ${id}` },
      style: denseStyle,
    });
  }
  const denseRows = await guiStyleRows(client, entity);
  expect(
    denseRows.size === 19,
    `Dense GuiRoot exposed ${denseRows.size} node style rows`,
  );
  const lastBackground = denseRows.get(19)?.background_color;
  expect(
    Array.isArray(lastBackground) && lastBackground.length === 4,
    "Dense GuiRoot inspection omitted the last node's style",
  );
  expect(
    Object.keys((await guiProperties(client, entity)).properties).length === 0,
    "Node style was also stored as named properties",
  );
  for (let id = 19; id >= 4; id--) {
    await client.editGui({ action: "remove", handle: handle(id) });
  }

  // Long extension property names make one panel's descriptor table exceed
  // the former 64 KiB byte bound. Beyond the message budget, inspection fails
  // explicitly without truncating state, and the same connection remains
  // usable.
  const denseRef = { kind: "alias", alias: 81 } as const;
  const denseEntity = aliasId(
    await client.batch([
      createEntity(81, "gui-dense-panel"),
      insertComponent(client, "Surface", denseRef, { width: 4, height: 3 }),
      insertComponent(client, "GuiRoot", denseRef),
    ]),
    81,
  );
  await client.editGui({
    action: "insert",
    entity: denseEntity,
    rootIncarnation: (await client.inspectGui({ entity: denseEntity }))
      .rootIncarnation,
    id: 1,
    index: 0,
    data: { kind: "container", containerKind: "column" },
  });
  const densePart = (index: number) => `dense_${index}_${"x".repeat(4000)}`;
  const setDenseProperties = async (from: number, to: number) => {
    for (let start = from; start < to; start += 50)
      successfulBatch(
        await client.batch(
          Array.from({ length: Math.min(50, to - start) }, (_, offset) => ({
            kind: "setDynamicProperty" as const,
            entity: { kind: "handle" as const, id: denseEntity },
            component: client.components.GuiRoot!.id,
            name: densePart(start + offset),
            value: { kind: "vec4" as const, value: [0.1, 0.2, 0.3, 1] },
          })),
        ),
      );
  };
  const denseProperties = async () => {
    const snapshot = (await client.inspect()).entities.find(
      (item) => item.id === denseEntity,
    );
    expect(snapshot, "Dense GUI entity disappeared");
    return [snapshot.base, snapshot.effective].map((components) =>
      Object.keys(
        components.find(
          (item) => item.component === client.components.GuiRoot!.id,
        )?.properties ?? {},
      ).filter((name) => name.startsWith("dense_")),
    );
  };
  await setDenseProperties(0, 20);
  const encoder = new TextEncoder();
  const [denseBase, denseEffective] = await denseProperties();
  // UTF-8 property names alone bound the descriptor table from below.
  const denseNameBytes = denseEffective!.reduce(
    (total, name) => total + encoder.encode(name).length,
    0,
  );
  expect(
    denseNameBytes > 65536 &&
      denseBase!.length === 20 &&
      denseEffective!.length === 20 &&
      denseEffective!.includes(densePart(19)),
    `Dense inspection decoded ${denseBase!.length}/${denseEffective!.length} properties and ${denseNameBytes} name bytes`,
  );
  await setDenseProperties(20, 300);
  let oversized: unknown;
  try {
    await client.inspect();
  } catch (error) {
    oversized = error;
  }
  expect(
    oversized instanceof Error &&
      /Inspection record cannot be encoded/.test(oversized.message),
    `An oversized inspection record was not rejected explicitly: ${String(oversized)}`,
  );
  // The same connection serves targeted reads and edits; state was not truncated.
  expect(
    (await client.inspectGui({ entity: denseEntity })).nodes.length === 1,
    "Oversized inspection changed the GUI tree",
  );
  for (let start = 20; start < 300; start += 50)
    successfulBatch(
      await client.batch(
        Array.from({ length: Math.min(50, 300 - start) }, (_, offset) => ({
          kind: "removeDynamicProperty" as const,
          entity: { kind: "handle" as const, id: denseEntity },
          component: client.components.GuiRoot!.id,
          name: densePart(start + offset),
        })),
      ),
    );
  const [restoredBase] = await denseProperties();
  expect(
    restoredBase!.length === 20 && restoredBase!.includes(densePart(19)),
    "Inspection after the oversized record lost or truncated named properties",
  );
  successfulBatch(
    await client.batch([
      { kind: "delete", entity: { kind: "handle", id: denseEntity } },
    ]),
  );

  await client.editGui({
    action: "move",
    handle: handle(3),
    parent: 1,
    index: 0,
  });
  const moved = await client.inspectGui({ entity });
  expect(
    moved.nodes[0]!.children.join() === "3,2",
    "Reordering must retain node identities",
  );

  // Revision-gated committed values reject stale writers.
  await client.editGui({
    action: "setControlValue",
    handle: handle(3),
    expectedRevision: 1,
    value: { kind: "scalar", value: 0.75 },
  });
  await rejects(
    client.editGui({
      action: "setControlValue",
      handle: handle(3),
      expectedRevision: 1,
      value: { kind: "scalar", value: 0.1 },
    }),
    "A stale control revision was accepted",
  );
  const slider = (await client.inspectGui({ entity, nodeId: 3, maxDepth: 1 }))
    .nodes[0]!;
  expect(
    slider.controlValue.kind === "scalar" &&
      slider.controlValue.value === 0.75 &&
      slider.controlRevision === 2,
    `Committed slider ${JSON.stringify(slider.controlValue)}@${slider.controlRevision}`,
  );

  // A write delayed across control -> non-control -> control transitions of the
  // same node is still fenced by one monotonic revision.
  const [, , delayed] = await Promise.allSettled([
    client.editGui({
      action: "update",
      handle: handle(3),
      patch: { data: { kind: "text", text: "paused" } },
    }),
    client.editGui({
      action: "update",
      handle: handle(3),
      patch: {
        data: { kind: "slider" },
        values: { value: 0.25, min: 0, max: 1, step: 0 },
      },
    }),
    client.editGui({
      action: "setControlValue",
      handle: handle(3),
      expectedRevision: 2,
      value: { kind: "scalar", value: 0.1 },
    }),
  ]);
  expect(
    delayed.status === "rejected",
    "A stale write applied to a re-created control",
  );
  await client.editGui({
    action: "setControlValue",
    handle: handle(3),
    expectedRevision: 4,
    value: { kind: "scalar", value: 0.75 },
  });

  // A partial style patch preserves omitted properties.
  await client.editGui({
    action: "update",
    handle: handle(2),
    patch: { style: { opacity: 0.5 } },
  });
  const checkbox = (await client.inspectGui({ entity, nodeId: 2, maxDepth: 1 }))
    .nodes[0]!;
  expect(
    checkbox.style.opacity === 0.5 &&
      Math.abs(checkbox.style.fontSize! - 0.2) < 1e-6 &&
      Math.abs(checkbox.style.color![1] - 0.4) < 1e-6,
    `Style patch replaced omitted properties: ${JSON.stringify(checkbox.style)}`,
  );

  // The tree is a rows table whose derived child order matches inspection;
  // reparenting through a generic write or adding raw Surface items is
  // rejected.
  const tree = (await guiProperties(client, entity)).fields.node_tree as
    | { rows: ReadonlyMap<number, NodeTreeRow> }
    | undefined;
  expect(tree?.rows instanceof Map, "GuiRoot inspection omitted node_tree");
  expect(tree.rows.size === 3, "Inspected GuiRoot tree rows do not decode");
  const derived = guiTreeChildren(tree.rows);
  expect(
    [1, ...(derived.get(1) ?? [])].join(",") ===
      ids(await client.inspectGui({ entity })),
    `Tree rows derive child order ${JSON.stringify([...derived])}`,
  );
  const structural = await client.batch([
    {
      kind: "setField",
      entity: { kind: "handle", id: entity },
      component: client.components.GuiRoot!.id,
      field: {
        offset: GuiRoot.node_treeOffset(2, "parent"),
        value: { kind: "dynamic", value: { kind: "u32", value: 3 } },
      },
    },
  ]);
  expect(!structural.ok, "A live GUI tree accepted a generic field write");
  await rejects(
    client.editSurface({
      action: "insert",
      entity,
      id: 1,
      index: 0,
      content: { kind: "label", text: "raw" },
      style: {},
    }),
    "Raw Surface items were accepted on a GUI-owned Surface",
  );
  expect(
    ids(await client.inspectGui({ entity })) === "1,3,2",
    "Rejected writes changed the tree",
  );

  // Part rows die with their node; stale handles cannot retarget.
  await client.editGui({
    action: "updatePart",
    handle: handle(2),
    part: "background",
    patch: { color: [1, 0, 0, 1] },
  });
  const partNodes = async () => {
    const table = (await guiProperties(client, entity)).fields.part_state as
      | { rows: ReadonlyMap<number, Readonly<Record<string, unknown>>> }
      | undefined;
    expect(table?.rows instanceof Map, "GuiRoot inspection omitted part_state");
    return [...table.rows.values()].map((row) => row.node);
  };
  expect(
    (await partNodes()).includes(2),
    "A part override did not create the node's part row",
  );
  await client.editGui({ action: "remove", handle: handle(2) });
  const remaining = await guiStyleRows(client, entity);
  const parts = await partNodes();
  expect(
    !parts.includes(2) && !remaining.has(2),
    `Removed node rows survived: parts ${parts.join()}`,
  );
  await rejects(
    client.editGui({
      action: "update",
      handle: handle(2),
      patch: { style: { opacity: 1 } },
    }),
    "A removed node handle was accepted",
  );
  await rejects(
    client.editGui({
      action: "insert",
      entity,
      rootIncarnation,
      id: 2,
      parent: 1,
      index: 0,
      data: { kind: "text", text: "reused" },
    }),
    "A removed node identity was reused",
  );

  // Overlays cannot write out-of-range GUI properties, and raw items hidden by an
  // overlay keep GUI ownership from being acquired until they are gone.
  const gui = client.components.GuiRoot!.id;
  const overlay = (
    symbolicId: string,
    component: number,
    fields: ReturnType<typeof componentFields>,
  ) =>
    [
      { kind: "createStateOverlayOwner", alias: 1 },
      {
        kind: "attachEntityOverlayBinding",
        owner: { kind: "alias", alias: 1 },
        alias: 2,
        symbolicId,
        mode: "bound",
      },
      {
        kind: "attachComponentStateOverlay",
        owner: { kind: "alias", alias: 1 },
        binding: { kind: "alias", alias: 2 },
        alias: 3,
        component,
        mode: "bound",
        fields,
      },
    ] as const;
  const invalid = await client.batch([
    ...overlay("gui-panel", gui, [
      {
        offset: GuiRoot.node_styleOffset(3, "opacity"),
        value: { kind: "dynamic", value: { kind: "f32", value: 2 } },
      },
    ]),
  ]);
  expect(!invalid.ok, "An out-of-range GUI overlay value was accepted");
  const opacity = (await guiStyleRows(client, entity)).get(3)?.opacity;
  expect(
    opacity === 1,
    `Rejected overlay changed opacity: ${JSON.stringify(opacity)}`,
  );

  const hidden = aliasId(
    await client.batch([
      createEntity(1, "gui-hidden"),
      insertComponent(client, "Surface", ref, { width: 2, height: 1 }),
    ]),
    1,
  );
  await client.editSurface({
    action: "insert",
    entity: hidden,
    id: 1,
    index: 0,
    content: { kind: "drawing" },
    style: {},
  });
  const hiding = successfulBatch(
    await client.batch([
      ...overlay(
        "gui-hidden",
        client.components.Surface!.id,
        componentFields(client, "Surface", {
          items: client.encodeSurfaceItems({ nextId: 1, items: [] }),
        }),
      ),
    ]),
  );
  const attach = await client.batch([
    insertComponent(client, "GuiRoot", { kind: "handle", id: hidden }),
  ]);
  expect(!attach.ok, "GUI ownership was acquired over hidden raw items");
  successfulBatch(
    await client.batch([
      {
        kind: "releaseStateOverlayOwner",
        owner: { kind: "handle", id: hiding.stateOverlays[0]!.id },
      },
    ]),
  );
  const restoredItems = (await client.inspect()).entities
    .find((item) => item.id === hidden)!
    .effective.find((item) => item.component === client.components.Surface!.id)!
    .fields.items;
  expect(
    restoredItems instanceof Uint8Array &&
      client.decodeSurfaceItems(restoredItems).items.length === 1,
    "Releasing the overlay did not restore the raw items",
  );
  expect(
    !(await client.inspect()).entities
      .find((item) => item.id === hidden)!
      .effective.some((item) => item.component === gui),
    "A GuiRoot coexists with raw Surface items",
  );

  // Each root owns its density. Authored lanes are logical units, so the
  // 1 x 0.5 box keeps its logical bounds while the root filling the 4 x 3 m
  // Surface spans (4U, 3U); writes, insertion values and overlays all reflow
  // only their own root, and invalid values are rejected.
  const densityRoot = async (alias: number, units?: number) => {
    const densityRef = { kind: "alias", alias } as const;
    const densityEntity = aliasId(
      await client.batch([
        createEntity(alias, `gui-density-${alias}`),
        insertComponent(client, "Surface", densityRef, {
          width: 4,
          height: 3,
        }),
        insertComponent(
          client,
          "GuiRoot",
          densityRef,
          units === undefined ? {} : { units_per_metre: units },
        ),
      ]),
      alias,
    );
    const incarnation = (await client.inspectGui({ entity: densityEntity }))
      .rootIncarnation;
    await client.editGuiBatch([
      {
        action: "insert",
        entity: densityEntity,
        rootIncarnation: incarnation,
        id: 1,
        index: 0,
        data: { kind: "container", containerKind: "column" },
      },
      {
        action: "insert",
        entity: densityEntity,
        rootIncarnation: incarnation,
        id: 2,
        parent: 1,
        index: 0,
        data: { kind: "container", containerKind: "sizedBox" },
        style: { width: 1, height: 0.5 },
      },
    ]);
    return densityEntity;
  };
  const boxBounds = async (densityEntity: bigint) => {
    await client.waitForFrame();
    const tree = await client.semanticSnapshot({ entity: densityEntity });
    const root = tree.nodes.find((node) => node.id === 1);
    const box = tree.nodes.find((node) => node.id === 2);
    expect(root && box, "Density panel omitted its root or box");
    return `${root.bounds.join()}|${box.bounds.join()}`;
  };
  const densityValue = async (densityEntity: bigint) => {
    const snapshot = (await client.inspect()).entities.find(
      (item) => item.id === densityEntity,
    );
    expect(snapshot, "Density panel disappeared");
    const read = (components: typeof snapshot.effective) =>
      components.find(
        (item) => item.component === client.components.GuiRoot!.id,
      )?.fields.units_per_metre;
    return [read(snapshot.base), read(snapshot.effective)].join();
  };
  const densityWrite = (densityEntity: bigint, units: number) => ({
    kind: "setField" as const,
    entity: { kind: "handle" as const, id: densityEntity },
    component: client.components.GuiRoot!.id,
    field: componentFields(client, "GuiRoot", { units_per_metre: units })[0]!,
  });
  const plainDensity = await densityRoot(90);
  const denseDensity = await densityRoot(91, 2);
  expect(
    (await boxBounds(plainDensity)) === "0,0,4,3|0,0,1,0.5" &&
      (await boxBounds(denseDensity)) === "0,0,8,6|0,0,1,0.5",
    "Per-root densities did not scale evaluated bounds independently",
  );
  successfulBatch(await client.batch([densityWrite(plainDensity, 0.5)]));
  expect(
    (await boxBounds(plainDensity)) === "0,0,2,1.5|0,0,1,0.5" &&
      (await boxBounds(denseDensity)) === "0,0,8,6|0,0,1,0.5",
    "A density write did not reflow only its own root",
  );
  for (const units of [0, -1]) {
    expect(
      !(await client.batch([densityWrite(plainDensity, units)])).ok,
      `An invalid density ${units} was accepted`,
    );
  }
  const densityOverlay = successfulBatch(
    await client.batch([
      ...overlay(
        "gui-density-91",
        gui,
        componentFields(client, "GuiRoot", { units_per_metre: 4 }),
      ),
    ]),
  );
  expect(
    (await boxBounds(denseDensity)) === "0,0,16,12|0,0,1,0.5" &&
      (await densityValue(denseDensity)) === "2,4",
    "A density overlay did not reflow the root over its authored value",
  );
  successfulBatch(
    await client.batch([
      {
        kind: "releaseStateOverlayOwner",
        owner: { kind: "handle", id: densityOverlay.stateOverlays[0]!.id },
      },
    ]),
  );
  expect(
    (await boxBounds(denseDensity)) === "0,0,8,6|0,0,1,0.5" &&
      (await densityValue(denseDensity)) === "2,2",
    "Releasing a density overlay did not restore the authored value",
  );

  // Persistence keeps structure and committed values; old handles stay fenced.
  const before = await client.inspectGui({ entity });
  const bytes = await host.saveWorld();
  await host.detachWorld();
  const restored = await host.loadWorld(bytes, { symbolicId: "gui-restored" });
  const panel = (await restored.inspect()).entities.find(
    (item) => item.metadata.symbolicId === "gui-panel",
  );
  expect(panel, "Restored World omitted the GUI panel");
  const after = await restored.inspectGui({ entity: panel.id });
  expect(ids(after) === ids(before), `Restored tree ${ids(after)}`);
  const restoredDensity = (await restored.inspect()).entities.find(
    (item) => item.metadata.symbolicId === "gui-density-90",
  );
  expect(restoredDensity, "Restored World omitted the density panel");
  const restoredUnits = restoredDensity.effective.find(
    (item) => item.component === restored.components.GuiRoot!.id,
  )?.fields.units_per_metre;
  await restored.waitForFrame();
  const restoredRoot = (
    await restored.semanticSnapshot({ entity: restoredDensity.id })
  ).nodes.find((node) => node.id === 1);
  expect(
    restoredUnits === 0.5 && restoredRoot?.bounds.join() === "0,0,2,1.5",
    `Restored density ${String(restoredUnits)} evaluated ${String(restoredRoot?.bounds)}`,
  );
  const restoredSlider = after.nodes.find((node) => node.id === 3)!;
  expect(
    restoredSlider.controlValue.kind === "scalar" &&
      restoredSlider.controlValue.value === 0.75 &&
      restoredSlider.controlRevision === 5,
    "Restored World lost the committed slider value",
  );
  await rejects(
    restored.editGui({
      action: "remove",
      handle: handle(3),
    }),
    "A handle from the replaced session was accepted",
  );
  await restored.editGui({
    action: "setControlValue",
    handle: restored.createGuiNodeHandle(panel.id, after.rootIncarnation, 3),
    expectedRevision: 5,
    value: { kind: "scalar", value: 0.5 },
  });
  return {
    batchApplied: batchOutcome.applied,
    failedBatchApplied: failedBatch.applied,
    largeBatchApplied: largeOutcome.applied,
    failedLargeBatchApplied: failedLargeOutcome.applied,
    rootIncarnation: String(rootIncarnation),
    restoredIncarnation: String(after.rootIncarnation),
    nodes: ids(after),
    denseStyleRows: denseRows.size,
    denseNameBytes,
  };
}

/** Wait until the World's only asset, a TextInput font, has loaded: text
 * inputs take focus only against a ready font. */
export async function loadedFont(client: GuiTestClient): Promise<void> {
  for (let attempt = 0; ; attempt += 1) {
    const resources = (await client.inspect()).resources;
    if (resources.some((item) => item.status === "loaded")) return;
    expect(attempt < 200, "The TextInput font never loaded");
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
}

/** Completed RGBA frame of the environment's presentation, top row first. */
export interface GuiFrame {
  readonly width: number;
  readonly height: number;
  readonly pixels: ArrayBuffer;
}

/** Capture the next completed frame of a World with an attached canvas. */
export type GuiFrameCapture = (client: GuiTestClient) => Promise<GuiFrame>;

/** Pixels whose channels differ by more than `tolerance` between frames. */
function changedPixels(a: GuiFrame, b: GuiFrame, tolerance = 2): number {
  expect(
    a.width === b.width && a.height === b.height,
    "Compared frames differ in size",
  );
  const left = new Uint8Array(a.pixels);
  const right = new Uint8Array(b.pixels);
  let changed = 0;
  for (let offset = 0; offset < left.length; offset += 4)
    for (let channel = 0; channel < 3; channel += 1)
      if (
        Math.abs(left[offset + channel]! - right[offset + channel]!) > tolerance
      ) {
        changed += 1;
        break;
      }
  return changed;
}

/** Pixels that differ from the frame's top-left background pixel. */
function contentPixels(frame: GuiFrame): number {
  const pixels = new Uint8Array(frame.pixels);
  let content = 0;
  for (let offset = 0; offset < pixels.length; offset += 4)
    for (let channel = 0; channel < 3; channel += 1)
      if (Math.abs(pixels[offset + channel]! - pixels[channel]!) > 2) {
        content += 1;
        break;
      }
  return content;
}

/** Capture until two consecutive completed frames agree, so glyph and
 * atlas work from earlier frames has settled. */
async function settledFrame(
  client: GuiTestClient,
  capture: GuiFrameCapture,
): Promise<GuiFrame> {
  let previous = await capture(client);
  for (let attempt = 0; attempt < 60; attempt += 1) {
    const next = await capture(client);
    if (changedPixels(previous, next, 0) === 0) return next;
    previous = next;
  }
  throw new Error("GUI presentation did not settle");
}

/** Camera of {@link activatePanelCamera}: distance and vertical field of view. */
const PANEL_CAMERA = { distance: 5, fovY: ORTHOGRAPHIC_CAMERA.fov_y } as const;

/** Normalized top-left viewport point of a logical point on the centred
 * 4x3 panel, seen by the panel camera in a frame of this aspect (one
 * logical unit per metre). */
function panelViewportPoint(
  frame: GuiFrame,
  [x, y]: [number, number],
): [number, number] {
  const halfHeight = PANEL_CAMERA.distance * Math.tan(PANEL_CAMERA.fovY / 2);
  const halfWidth = (halfHeight * frame.width) / frame.height;
  return [0.5 * (1 + (x - 2) / halfWidth), 0.5 * (1 - (1.5 - y) / halfHeight)];
}

/** Author a perspective camera facing the Surface front from 5 m on +Z. */
async function activatePanelCamera(client: GuiTestClient) {
  const camera = { kind: "alias", alias: 70 } as const;
  const id = aliasId(
    await client.batch([
      createEntity(70, "gui-restore-camera"),
      insertComponent(client, "Transform", camera, {
        z: PANEL_CAMERA.distance,
      }),
      insertComponent(client, "Camera", camera, {
        ...ORTHOGRAPHIC_CAMERA,
        projection: 0,
      }),
    ]),
    70,
  );
  cameraClient(client).sendCommand({
    type: "CameraActivateCommand",
    entity: id,
  });
}

/**
 * Save a World while a TextInput holds focus, a selection and an open
 * composition and a checkbox holds a pointer press, then restore it: the
 * restored World keeps the structure, committed values and part overrides,
 * holds no focus, capture, selection or composition, and takes only fresh
 * handles. With `capture`, the restored panel's completed frame matches the
 * frame of the same committed state before any interaction, while the
 * interacting frame differs from both.
 *
 * The 4x3 panel stacks a checkbox, a TextInput and a slider, one unit each.
 */
export async function exerciseGuiTransientRestore(
  host: WorldPersistenceHostClient<GuiTestClient>,
  fontBytes: ArrayBuffer,
  capture?: GuiFrameCapture,
) {
  const client = await host.createWorld({ symbolicId: "gui-transient" });
  const font = await client.createAsset(17, fontBytes);
  const ref = { kind: "alias", alias: 1 } as const;
  const entity = aliasId(
    await client.batch([
      createEntity(1, "gui-transient-panel"),
      insertComponent(client, "Transform", ref),
      insertComponent(client, "Surface", ref, { width: 4, height: 3 }),
      insertComponent(client, "GuiRoot", ref),
    ]),
    1,
  );
  const { rootIncarnation } = await client.inspectGui({ entity });
  const row = { width: 4, height: 1 } as const;
  await client.editGuiBatch([
    {
      action: "insert",
      entity,
      rootIncarnation,
      id: 1,
      index: 0,
      data: { kind: "container", containerKind: "column" },
      style: { width: 4, height: 3 },
    },
    {
      action: "insert",
      entity,
      rootIncarnation,
      id: 2,
      parent: 1,
      index: 0,
      data: { kind: "checkbox" },
      values: { checked: true },
      style: row,
    },
    {
      action: "insert",
      entity,
      rootIncarnation,
      id: 3,
      parent: 1,
      index: 1,
      data: { kind: "textInput", text: "ab", placeholder: "" },
      style: { ...row, fontSize: 0.6, asset: font },
    },
    {
      action: "insert",
      entity,
      rootIncarnation,
      id: 4,
      parent: 1,
      index: 2,
      data: { kind: "slider" },
      values: { value: 0.75, min: 0, max: 1, step: 0 },
      style: row,
    },
    {
      action: "updatePart",
      handle: client.createGuiNodeHandle(entity, rootIncarnation, 2),
      part: "background",
      patch: { color: [0.9, 0.2, 0.1, 1] },
    },
  ]);
  if (capture) await activatePanelCamera(client);
  // Programmatic focus is fenced against evaluated layout with a ready font.
  await loadedFont(client);
  await client.waitForFrame();
  const committed = capture ? await settledFrame(client, capture) : undefined;
  // Without a camera, pointers address panel logical units; through the
  // active camera they are normalized viewport points projected onto the
  // panel.
  const checkboxPoint = committed
    ? panelViewportPoint(committed, [2, 0.5])
    : ([2, 0.5] as [number, number]);

  // Interaction state: a held press capturing pointer 1 on the checkbox,
  // then focus, selection and composition on the TextInput.
  const handle = (id: number) =>
    client.createGuiNodeHandle(entity, rootIncarnation, id);
  const states: (GuiTextFocusState | null)[] = [];
  const stop = client.subscribeGuiObservations((batch) => {
    if (batch.textFocus !== undefined) states.push(batch.textFocus);
  });
  // A press focuses its control, so the checkbox press comes first and
  // keeps capturing pointer 1 while focus moves to the TextInput.
  const press = await client.submitGuiInput({
    kind: "pointerDown",
    pointer: 1,
    position: checkboxPoint,
    button: "primary",
  });
  const focus = await client.submitGuiInput({
    kind: "focus",
    handle: handle(3),
  });
  await client.submitGuiInput({ kind: "setTextSelection", start: 0, end: 1 });
  await client.submitGuiInput({
    kind: "composition",
    text: "zz",
    caretStart: 2,
    caretEnd: 2,
  });
  expect(
    focus.unhandled === undefined && press.unhandled === undefined,
    `Interaction before save was not routed: ${JSON.stringify([focus.unhandled, press.unhandled])}`,
  );
  const deadline = Date.now() + 10000;
  while (states.at(-1)?.composition?.text !== "zz") {
    expect(Date.now() < deadline, "The TextInput published no composition");
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  stop();
  const interacting = capture ? await settledFrame(client, capture) : undefined;
  const before = await client.inspectGui({ entity });

  const bytes = await host.saveWorld();
  await host.detachWorld();
  const restored = await host.loadWorld(bytes, {
    symbolicId: "gui-transient-restored",
  });
  const panel = (await restored.inspect()).entities.find(
    (item) => item.metadata.symbolicId === "gui-transient-panel",
  );
  expect(panel, "Restored World omitted the transient panel");
  const after = await restored.inspectGui({ entity: panel.id });
  const values = (response: GuiInspectResponse) =>
    JSON.stringify(response.nodes.map((node) => [node.id, node.controlValue]));
  expect(
    values(after) === values(before),
    `Restored committed values ${values(after)} differ from ${values(before)}`,
  );
  const partOverride = (
    (await guiProperties(restored, panel.id)).fields.part_state as
      | { rows: ReadonlyMap<number, Readonly<Record<string, unknown>>> }
      | undefined
  )?.rows;
  expect(
    [...(partOverride?.values() ?? [])].some((row) => row.node === 2),
    "Restored World lost the checkbox part override",
  );

  // No focus, capture, selection or composition survived the save.
  const restoredStates: (GuiTextFocusState | null)[] = [];
  const restoredStop = restored.subscribeGuiObservations((batch) => {
    if (batch.textFocus !== undefined) restoredStates.push(batch.textFocus);
  });
  await restored.waitForFrame();
  const snapshot = await restored.semanticSnapshot({ entity: panel.id });
  const release = await restored.submitGuiInput({
    kind: "pointerUp",
    pointer: 1,
    position: checkboxPoint,
    button: "primary",
  });
  const commit = await restored.submitGuiInput({ kind: "commitComposition" });
  const typed = await restored.submitGuiInput({ kind: "text", text: "x" });
  const unchanged = await restored.inspectGui({ entity: panel.id });
  expect(
    snapshot.focused === undefined &&
      release.unhandled?.kind === "noCapture" &&
      commit.unhandled?.kind === "noFocus" &&
      typed.unhandled?.kind === "noFocus" &&
      values(unchanged) === values(before),
    `Restored World kept interaction state: ${JSON.stringify({ focused: snapshot.focused, release: release.unhandled, commit: commit.unhandled, typed: typed.unhandled, values: values(unchanged) })}`,
  );

  // Handles from the saved session are rejected; fresh handles work, and
  // focusing the restored TextInput starts with a collapsed selection and
  // no composition.
  const staleFocus = await restored
    .submitGuiInput({ kind: "focus", handle: handle(3) })
    .then(
      (reply) => reply.unhandled?.kind ?? "handled",
      () => "rejected",
    );
  expect(
    staleFocus !== "handled",
    "A handle from the saved session focused the restored TextInput",
  );
  await rejects(
    restored.editGui({
      action: "update",
      handle: handle(2),
      patch: { style: { opacity: 0.5 } },
    }),
    "A handle from the saved session edited the restored World",
  );
  const fresh = await restored.submitGuiInput({
    kind: "focus",
    handle: restored.createGuiNodeHandle(panel.id, after.rootIncarnation, 3),
  });
  expect(fresh.unhandled === undefined, "A fresh handle did not focus");
  const restoredDeadline = Date.now() + 10000;
  while (restoredStates.at(-1)?.node !== 3) {
    expect(
      Date.now() < restoredDeadline,
      "The restored TextInput published no text focus",
    );
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  restoredStop();
  const refocused = restoredStates.at(-1)!;
  expect(
    refocused.text === "ab" &&
      refocused.composition === undefined &&
      refocused.selectionStart === refocused.selectionEnd,
    `Refocused restored TextInput kept transient text state: ${JSON.stringify(refocused, (_, value) => (typeof value === "bigint" ? `${value}` : value))}`,
  );

  // The restored frame shows the committed values and part override exactly
  // as before any interaction; the interacting frame differs from both.
  let frames: Record<string, number> | undefined;
  if (capture) {
    await restored.submitGuiInput({ kind: "blur" });
    const camera = (await restored.inspect()).entities.find(
      (item) => item.metadata.symbolicId === "gui-restore-camera",
    );
    expect(camera, "Restored World omitted the camera");
    cameraClient(restored).sendCommand({
      type: "CameraActivateCommand",
      entity: camera.id,
    });
    await restored.waitForFrame();
    const restoredFrame = await settledFrame(restored, capture);
    const interactionPixels = changedPixels(committed!, interacting!);
    const restoredPixels = changedPixels(committed!, restoredFrame);
    const panelPixels = contentPixels(restoredFrame);
    expect(
      panelPixels > 1000 && interactionPixels > 50 && restoredPixels === 0,
      `Restored frame does not match the committed frame: ${JSON.stringify({ panelPixels, interactionPixels, restoredPixels })}`,
    );
    frames = { panelPixels, interactionPixels, restoredPixels };
  }
  await host.detachWorld();
  return {
    nodes: ids(after),
    values: values(after),
    restoredIncarnation: String(after.rootIncarnation),
    release: release.unhandled?.kind,
    commit: commit.unhandled?.kind,
    staleFocus,
    frames: frames ?? "no presentation in this environment",
  };
}
