/** GUI root and node lifecycle through a generated client, independent of process launch and wire layout. */
import type {
  AssetWorldClient,
  GuiInspectResponse,
  GuiWorldClient,
  SurfaceWorldClient,
  WorldPersistenceHostClient,
  guiPartProperty,
  guiProperty,
} from "@ipp/client";
import {
  aliasId,
  componentFields,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";

export type GuiTestClient = GuiWorldClient &
  SurfaceWorldClient &
  AssetWorldClient;

/** Property naming exported by the generated contract under test. */
export interface GuiContractNames {
  guiProperty: typeof guiProperty;
  guiPartProperty: typeof guiPartProperty;
}

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

/**
 * Exercise root identity, pipelined edits, revision-gated values, ownership
 * rejection, property invalidation and persistence against a live World.
 */
export async function exerciseGuiLifecycle(
  host: WorldPersistenceHostClient<GuiTestClient>,
  { guiProperty, guiPartProperty }: GuiContractNames,
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
          content: {
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
          content: { kind: "text" as const, text: `node ${index}` },
        },
  );
  const batchOutcome = await client.editGuiBatch(batchEdits);
  expect(batchOutcome.ok, "A valid 50-node GUI batch was rejected");
  expect(
    batchOutcome.applied === 50,
    "The GUI batch acknowledged a short prefix",
  );
  expect(
    batchOutcome.requests === 1,
    "The 50-node GUI batch used multiple requests",
  );
  await client.waitForFrame();
  let batchedTree = await client.inspectGui({ entity: batchEntity });
  expect(
    batchedTree.nodes.length === 50,
    "The completed GUI batch frame omitted nodes",
  );
  const batchHandle = (id: number) =>
    client.createGuiNodeHandle(batchEntity, batchRoot.rootIncarnation, id, 1);
  const failedBatch = await client.editGuiBatch([
    {
      action: "update",
      handle: batchHandle(2),
      patch: { content: { kind: "text", text: "prefix applied" } },
    },
    {
      action: "insert",
      entity: batchEntity,
      rootIncarnation: batchRoot.rootIncarnation,
      id: 2,
      parent: 1,
      index: 0,
      content: { kind: "text", text: "must fail" },
    },
    {
      action: "update",
      handle: batchHandle(3),
      patch: { content: { kind: "text", text: "suffix must not apply" } },
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
    prefixNode?.content.kind === "text" &&
      prefixNode.content.text === "prefix applied",
    "The acknowledged GUI prefix was lost",
  );
  expect(
    suffixNode?.content.kind === "text" && suffixNode.content.text === "node 2",
    "A GUI edit after the failed operation was applied",
  );
  const recoveredBatch = await client.editGuiBatch([
    {
      action: "update",
      handle: batchHandle(3),
      patch: { content: { kind: "text", text: "recovered" } },
    },
  ]);
  expect(
    recoveredBatch.ok && recoveredBatch.applied === 1,
    "A correction after a failed GUI batch did not recover",
  );

  // Repeated large patches keep the final tree small while forcing the
  // generated client across the exact request-byte boundary.
  const largeUpdates = (character: string) =>
    Array.from({ length: 18 }, (_, index) => ({
      action: "update" as const,
      handle: batchHandle(2),
      patch: {
        content: {
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
      patch: { content: { kind: "text", text: "queued after multi-page" } },
    },
  ]);
  const queuedInspectionPromise = client.inspectGui({ entity: batchEntity });
  const largeOutcome = await largePromise;
  expect(
    largeOutcome.ok &&
      largeOutcome.applied === 18 &&
      largeOutcome.requests === 4,
    "A two-page GUI edit did not use begin, two buffers and finish",
  );
  const queuedDirect = await queuedDirectPromise;
  expect(
    queuedDirect.ok && queuedDirect.requests === 1,
    "A direct GUI edit queued behind a multi-page edit did not complete",
  );
  batchedTree = await queuedInspectionPromise;
  const largeNode = batchedTree.nodes.find((node) => node.id === 2);
  const queuedNode = batchedTree.nodes.find((node) => node.id === 3);
  expect(
    largeNode?.content.kind === "text" && largeNode.content.text.endsWith("17"),
    "The completed multi-page GUI edit lost its final value",
  );
  expect(
    queuedNode?.content.kind === "text" &&
      queuedNode.content.text === "queued after multi-page",
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
      content: { kind: "text" as const, text: "must fail" },
    },
    {
      action: "update" as const,
      handle: batchHandle(4),
      patch: {
        content: {
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
        content: { kind: "text", text: "multi-page recovered" },
      },
    },
  ]);
  const failedInspectionPromise = client.inspectGui({ entity: batchEntity });
  const failedLargeOutcome = await failedLargePromise;
  expect(
    !failedLargeOutcome.ok &&
      failedLargeOutcome.applied === 18 &&
      failedLargeOutcome.requests === 3,
    "A failed second GUI page lost its global acknowledged prefix",
  );
  const recoveredLargeOutcome = await recoveredLargePromise;
  expect(
    recoveredLargeOutcome.ok && recoveredLargeOutcome.requests === 1,
    "Queued recovery after a failed multi-page GUI edit did not complete",
  );
  batchedTree = await failedInspectionPromise;
  const failedLargePrefix = batchedTree.nodes.find((node) => node.id === 2);
  const failedLargeRecovery = batchedTree.nodes.find((node) => node.id === 3);
  const failedLargeSuffix = batchedTree.nodes.find((node) => node.id === 4);
  expect(
    failedLargePrefix?.content.kind === "text" &&
      failedLargePrefix.content.text.endsWith("17"),
    "The successful prefix on the failed second GUI page was lost",
  );
  expect(
    failedLargeSuffix?.content.kind === "text" &&
      failedLargeSuffix.content.text === "node 3",
    "A suffix after a failed second GUI page was applied",
  );
  expect(
    failedLargeRecovery?.content.kind === "text" &&
      failedLargeRecovery.content.text === "multi-page recovered",
    "An ordinary request overtook queued recovery after batch failure",
  );
  const reusedGateOutcome = await client.editGuiBatch([
    {
      action: "update",
      handle: batchHandle(3),
      patch: {
        content: { kind: "text", text: "automatic gate reused" },
      },
    },
  ]);
  expect(
    reusedGateOutcome.ok && reusedGateOutcome.requests === 1,
    "The automatic GUI gate could not be reused after queued recovery",
  );

  // Observe the real Host between explicit GUI buffers. The held World's
  // frame and inspection cannot complete, while a separate World progresses.
  const streamId = await client.beginBatch();
  const firstStreamPage = await client.editGuiBatchChunk(streamId, [
    {
      action: "update",
      handle: batchHandle(4),
      patch: { content: { kind: "text", text: "stream page one" } },
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
      patch: { content: { kind: "text", text: "stream page two" } },
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
    streamedFirst?.content.kind === "text" &&
      streamedFirst.content.text === "stream page one" &&
      streamedSecond?.content.kind === "text" &&
      streamedSecond.content.text === "stream page two",
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
    client.createGuiNodeHandle(entity, rootIncarnation, id, 1);

  // Edits and inspection pipelined in one ingress drain observe every edit.
  const [, , , pipelined] = await Promise.all([
    client.editGui({
      action: "insert",
      entity,
      rootIncarnation,
      id: 1,
      index: 0,
      content: { kind: "container", containerKind: "column" },
    }),
    client.editGui({
      action: "insert",
      entity,
      rootIncarnation,
      id: 2,
      parent: 1,
      index: 0,
      content: { kind: "checkbox", checked: false },
      style: { color: [0.2, 0.4, 0.6, 1], fontSize: 0.2 },
    }),
    client.editGui({
      action: "insert",
      entity,
      rootIncarnation,
      id: 3,
      parent: 1,
      index: 1,
      content: { kind: "slider", value: 0.25, min: 0, max: 1, step: 0 },
    }),
    client.inspectGui({ entity }),
  ]);
  expect(ids(pipelined) === "1,2,3", `Pipelined inspection ${ids(pipelined)}`);

  // A regular styled panel may have more than the legacy 256 snapshot fields:
  // inspection must preserve every named lane through the generated client.
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
      content: { kind: "text", text: `dense ${id}` },
      style: denseStyle,
    });
  }
  const denseProperties = (await guiProperties(client, entity)).properties;
  const densePropertyCount = Object.keys(denseProperties).length;
  expect(
    densePropertyCount > 256,
    `Dense GuiRoot exposed only ${densePropertyCount} named properties`,
  );
  expect(
    denseProperties[guiProperty(19, "background_color")]?.kind === "vec4",
    "Dense GuiRoot inspection omitted the last node's style",
  );
  for (let id = 19; id >= 4; id--) {
    await client.editGui({ action: "remove", handle: handle(id) });
  }

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
      patch: { content: { kind: "text", text: "paused" } },
    }),
    client.editGui({
      action: "update",
      handle: handle(3),
      patch: {
        content: { kind: "slider", value: 0.25, min: 0, max: 1, step: 0 },
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

  // A partial style patch preserves omitted lanes.
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
    `Style patch replaced omitted lanes: ${JSON.stringify(checkbox.style)}`,
  );

  // Clearing the tree through a generic write or adding raw Surface items is rejected.
  const tree = (await guiProperties(client, entity)).fields.nodes;
  expect(tree instanceof Uint8Array, "GuiRoot inspection omitted its tree");
  expect(
    client.decodeGuiTree(tree).nodes.length === 3,
    "Inspected GuiRoot tree does not decode",
  );
  const structural = await client.batch([
    {
      kind: "setField",
      entity: { kind: "handle", id: entity },
      component: client.components.GuiRoot!.id,
      field: componentFields(client, "GuiRoot", {
        nodes: client.encodeGuiTree({ nextId: 1, nodes: [] }),
      })[0]!,
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

  // Named-part properties die with their node; stale handles cannot retarget.
  const part = guiPartProperty(2, "background", "color");
  successfulBatch(
    await client.batch([
      {
        kind: "setDynamicProperty",
        entity: { kind: "handle", id: entity },
        component: client.components.GuiRoot!.id,
        name: part,
        value: { kind: "vec4", value: [1, 0, 0, 1] },
      },
    ]),
  );
  await client.editGui({ action: "remove", handle: handle(2) });
  const properties = (await guiProperties(client, entity)).properties;
  expect(
    !(part in properties) && !(guiProperty(2, "opacity") in properties),
    `Removed node properties survived: ${Object.keys(properties).join()}`,
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
      content: { kind: "text", text: "reused" },
    }),
    "A removed node identity was reused",
  );

  // Overlays cannot write out-of-range GUI lanes, and raw items hidden by an
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
    ...overlay("gui-panel", gui, []),
    {
      kind: "updateDynamicComponentStateOverlay",
      owner: { kind: "alias", alias: 1 },
      overlay: { kind: "alias", alias: 3 },
      properties: { [guiProperty(3, "opacity")]: { kind: "f32", value: 2 } },
      clear: [],
    },
  ]);
  expect(!invalid.ok, "An out-of-range GUI overlay value was accepted");
  const opacity = (await guiProperties(client, entity)).properties[
    guiProperty(3, "opacity")
  ];
  expect(
    opacity?.value === 1,
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
    handle: restored.createGuiNodeHandle(panel.id, after.rootIncarnation, 3, 1),
    expectedRevision: 5,
    value: { kind: "scalar", value: 0.5 },
  });
  return {
    batchApplied: batchOutcome.applied,
    batchRequests: batchOutcome.requests,
    failedBatchApplied: failedBatch.applied,
    largeBatchApplied: largeOutcome.applied,
    largeBatchRequests: largeOutcome.requests,
    failedLargeBatchApplied: failedLargeOutcome.applied,
    failedLargeBatchRequests: failedLargeOutcome.requests,
    rootIncarnation: String(rootIncarnation),
    restoredIncarnation: String(after.rootIncarnation),
    nodes: ids(after),
    densePropertyCount,
  };
}
