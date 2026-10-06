import type {
  AnimationWorldClient,
  PickingWorldClient,
  WorldPersistenceHostClient,
  ViewQueryTarget,
} from "@ipp/client";
import { clientAssetSource } from "../../../packages/ipp-client/src/asset-sources.js";
import { aliasId, successfulBatch } from "../../fixtures/commands.js";
import { check } from "../../harness/page/checks.js";

type Host = WorldPersistenceHostClient<AnimationWorldClient>;

function handle(id: bigint) {
  return { kind: "handle" as const, id };
}

async function rejected(action: () => Promise<unknown>, message: string) {
  try {
    await action();
  } catch (error) {
    check(error instanceof Error, message);
    return error.message;
  }
  throw new Error(message);
}

/** Shared real-transport admission and structural link case. */
export async function selectedWorldFoundation(host: Host) {
  const unknown = await rejected(
    () => host.createWorld({ selectedSystems: ["not.a.registered.factory"] }),
    "Unknown selected factory must reject",
  );
  check(
    unknown.includes("Unknown registered system"),
    "Unknown factory reason",
  );
  const missingDependency = await rejected(
    () => host.createWorld({ selectedSystems: ["ipp.camera"] }),
    "Registered camera without geometry must reject",
  );
  check(
    missingDependency.includes("ipp.geometry"),
    "Missing selected dependency reason",
  );

  const emptyWorld = await host.createWorld({ selectedSystems: [] });
  const empty = await host.openWorld(emptyWorld.reference);
  check(
    empty.manifest?.systems.length === 0,
    "Empty selection installed systems",
  );
  // Components every World admits whatever it selects.
  const coreComponents = empty.manifest.components.join();
  const emptyInspection = await empty.inspect();
  check(
    emptyInspection.tick > 0n &&
      emptyInspection.entities.length === 0 &&
      emptyInspection.resources.length === 0 &&
      emptyInspection.controllers?.length === 0 &&
      emptyInspection.renderDiagnostics.length === 0,
    "Aggregate inspection failed for an empty-system World",
  );
  const controllersRejection = await rejected(
    () => empty.inspectPage({ collection: "controllers" }),
    "Explicit omitted-controller inspection must reject",
  );
  check(
    controllersRejection.includes("Unsupported"),
    "Controller rejection reason",
  );
  await empty.close();

  const selectedWorld = await host.createWorld({
    symbolicId: "selected-links",
    selectedSystems: ["ipp.constraints"],
  });
  const client = await host.openWorld(selectedWorld.reference);
  check(client.manifest !== undefined, "Attached World omitted its manifest");
  check(
    client.manifest.systems.length === 1 &&
      client.manifest.systems[0] === "ipp.constraints",
    "Selected World advertised the compiled system set",
  );
  check(
    client.manifest.operations.includes("entityLinks"),
    "Entity links unavailable",
  );
  check(
    client.manifest.operations.includes("constraints"),
    "Selected constraints unavailable",
  );
  check(
    !client.manifest.operations.includes("camera"),
    "Omitted camera was advertised",
  );
  check(
    !client.manifest.operations.includes("geometry"),
    "Omitted geometry was advertised",
  );
  const scalar = client.components.Scalar;
  const mesh = client.components.MeshInstance;
  check(
    scalar !== undefined && mesh !== undefined,
    "Compiled descriptors missing",
  );
  check(
    client.manifest.components.includes(scalar.id),
    "Selected scalar unavailable",
  );
  check(
    !client.manifest.components.includes(mesh.id),
    "Omitted mesh advertised",
  );
  const queryClient = client as unknown as PickingWorldClient;
  const unsupportedView: ViewQueryTarget = {
    kind: "root",
    output: {
      world: selectedWorld.reference,
      entity: 0n,
      kind: "camera",
      incarnation: 0n,
    },
    expectedViewport: { width: 32, height: 32, devicePixelRatio: 1 },
  };
  const pickRejection = await rejected(
    () =>
      queryClient.query({
        type: "GeometryPickQuery",
        view: unsupportedView,
        x: 0.5,
        y: 0.5,
      }),
    "Omitted geometry query must reject",
  );
  check(pickRejection.includes("Unsupported"), "Geometry rejection reason");
  const projectRejection = await rejected(
    () =>
      queryClient.query({
        type: "CameraProjectQuery",
        view: unsupportedView,
        x: 0.5,
        y: 0.5,
        plane: { point: [0, 0, 0], normal: [0, 0, 1] },
      }),
    "Omitted camera query must reject",
  );
  check(projectRejection.includes("Unsupported"), "Camera rejection reason");
  const renderRejection = await rejected(
    () => client.inspectPage({ collection: "renderDiagnostics" }),
    "Omitted rendering inspection must reject",
  );
  check(renderRejection.includes("Unsupported"), "Rendering rejection reason");

  const created = successfulBatch(
    await client.batch([
      {
        kind: "create",
        alias: 1,
        metadata: { symbolicId: "link-parent", classes: [] },
      },
      {
        kind: "create",
        alias: 2,
        metadata: { symbolicId: "link-child", classes: [] },
      },
      {
        kind: "create",
        alias: 3,
        metadata: { symbolicId: "link-other", classes: [] },
      },
      {
        kind: "create",
        alias: 7,
        metadata: { symbolicId: "link-sibling", classes: [] },
      },
      {
        kind: "placeEntity",
        entity: { kind: "alias", alias: 7 },
        placement: { parent: { kind: "alias", alias: 1 }, before: null },
      },
      {
        kind: "placeEntity",
        entity: { kind: "alias", alias: 2 },
        placement: {
          parent: { kind: "alias", alias: 1 },
          before: { kind: "alias", alias: 7 },
        },
      },
    ]),
  );
  const parent = aliasId(created, 1);
  const child = aliasId(created, 2);
  const other = aliasId(created, 3);
  const sibling = aliasId(created, 7);

  const treeRoot = await client.inspectTreePage({ root: parent, maxDepth: 0 });
  check(
    treeRoot.nodes.length === 1 &&
      treeRoot.nodes[0]?.id === parent &&
      treeRoot.next === 0n,
    "Depth-zero tree read escaped its root",
  );
  const firstTreePage = await client.inspectTreePage({
    root: parent,
    maxDepth: 1,
    limit: 1,
  });
  check(
    firstTreePage.nodes[0]?.id === parent && firstTreePage.next === parent,
    "Bounded tree root page lost its cursor",
  );
  const secondTreePage = await client.inspectTreePage({
    root: parent,
    after: firstTreePage.next,
    maxDepth: 1,
    limit: 1,
  });
  check(
    secondTreePage.nodes[0]?.id === child &&
      secondTreePage.nodes[0]?.depth === 1 &&
      secondTreePage.next === child,
    "Tree continuation lost the first child",
  );
  const thirdTreePage = await client.inspectTreePage({
    root: parent,
    after: secondTreePage.next,
    maxDepth: 1,
    limit: 1,
  });
  check(
    thirdTreePage.nodes[0]?.id === sibling && thirdTreePage.next === 0n,
    "Tree continuation lost sibling order",
  );
  successfulBatch(
    await client.batch([
      {
        kind: "placeEntity",
        entity: handle(sibling),
        placement: {
          parent: handle(parent),
          before: handle(child),
        },
      },
    ]),
  );
  const reorderedTree = await client.inspectTreePage({
    root: parent,
    maxDepth: 1,
  });
  check(
    reorderedTree.nodes.map((node) => node.id).join() ===
      [parent, sibling, child].join() &&
      reorderedTree.nodes[1]!.order < reorderedTree.nodes[2]!.order,
    "Sibling reorder did not update the indexed tree",
  );
  const reorderedPage = await client.inspectTreePage({
    root: parent,
    maxDepth: 1,
    limit: 2,
  });
  const reorderedContinuation = await client.inspectTreePage({
    root: parent,
    after: reorderedPage.next,
    maxDepth: 1,
    limit: 2,
  });
  check(
    reorderedPage.nodes.map((node) => node.id).join() ===
      [parent, sibling].join() &&
      reorderedPage.next === sibling &&
      reorderedContinuation.nodes[0]?.id === child &&
      reorderedContinuation.next === 0n,
    "Reordered sibling pagination skipped or repeated a child",
  );
  const nested = successfulBatch(
    await client.batch([
      {
        kind: "create",
        alias: 10,
        metadata: { symbolicId: "link-grandchild", classes: [] },
      },
      {
        kind: "placeEntity",
        entity: { kind: "alias", alias: 10 },
        placement: { parent: handle(child), before: null },
      },
    ]),
  );
  const grandchild = aliasId(nested, 10);
  const shallowTree = await client.inspectTreePage({
    root: parent,
    maxDepth: 1,
  });
  const deepTree = await client.inspectTreePage({ root: parent, maxDepth: 2 });
  check(
    !shallowTree.nodes.some((node) => node.id === grandchild) &&
      deepTree.nodes.some((node) => node.id === grandchild && node.depth === 2),
    "Tree depth bound omitted or leaked a grandchild",
  );
  const outsideCursor = await rejected(
    () => client.inspectTreePage({ root: parent, after: other, maxDepth: 1 }),
    "Tree cursor outside subtree must reject",
  );
  check(outsideCursor.includes("outside"), "Outside cursor rejection reason");

  const inspected = await client.inspectPage({ collection: "entities" });
  const linked = inspected.entities.find((entry) => entry.id === child);
  const linkedSibling = inspected.entities.find(
    (entry) => entry.id === sibling,
  );
  check(
    linked?.link.parent === parent &&
      linkedSibling?.link.parent === parent &&
      linkedSibling.link.order < linked.link.order,
    "Sibling reorder lost full link order",
  );

  // A placement is an ordinary link write: the last one wins and the tree
  // follows it.
  successfulBatch(
    await client.batch([
      {
        kind: "placeEntity",
        entity: { kind: "symbol", symbol: "link-child" },
        placement: { parent: handle(other), before: null },
      },
    ]),
  );
  const moved = (
    await client.inspectPage({ collection: "entities" })
  ).entities.find((entry) => entry.id === child);
  check(moved?.link.parent === other, "Placement by symbol was not applied");
  const movedTree = await client.inspectTreePage({
    root: other,
    maxDepth: 1,
  });
  check(
    movedTree.nodes.some((node) => node.id === child && node.parent === other),
    "Placed link missing from tree",
  );

  const partial = await client.batch([
    {
      kind: "placeEntity",
      entity: handle(child),
      placement: { parent: null, before: null },
    },
    {
      kind: "insertComponent",
      entity: handle(child),
      component: mesh.id,
      fields: [],
    },
  ]);
  check(!partial.ok, "Unsupported selected component was accepted");
  check(
    partial.error.operation === 1,
    "Unsupported operation lost its batch index",
  );
  const afterPartial = (
    await client.inspectPage({ collection: "entities" })
  ).entities.find((entry) => entry.id === child);
  check(afterPartial?.link.parent === null, "Applied placement rolled back");

  successfulBatch(
    await client.batch([
      {
        kind: "placeEntity",
        entity: handle(child),
        placement: { parent: handle(parent), before: null },
      },
      { kind: "deleteSubtree", root: handle(parent) },
    ]),
  );
  const afterDelete = await client.inspectPage({ collection: "entities" });
  check(
    !afterDelete.entities.some(
      (entry) =>
        entry.id === parent ||
        entry.id === child ||
        entry.id === sibling ||
        entry.id === grandchild,
    ),
    "Subtree delete retained a descendant",
  );
  check(
    afterDelete.entities.some((entry) => entry.id === other),
    "Subtree delete removed an unrelated root",
  );
  const staleRoot = await rejected(
    () => client.inspectTreePage({ root: parent, maxDepth: 1 }),
    "Deleted tree root must reject",
  );
  check(staleRoot.includes("no longer exists"), "Stale root rejection reason");
  const retained = successfulBatch(
    await client.batch([
      {
        kind: "create",
        alias: 8,
        metadata: { symbolicId: "nonowning-parent", classes: [] },
      },
      {
        kind: "create",
        alias: 9,
        metadata: { symbolicId: "nonowning-child", classes: [] },
      },
      {
        kind: "placeEntity",
        entity: { kind: "alias", alias: 9 },
        placement: {
          parent: { kind: "alias", alias: 8 },
          before: null,
        },
      },
    ]),
  );
  const nonowningParent = aliasId(retained, 8);
  const nonowningChild = aliasId(retained, 9);
  successfulBatch(
    await client.batch([{ kind: "delete", entity: handle(nonowningParent) }]),
  );
  const forest = await client.inspectTreePage({ maxDepth: 0 });
  check(
    forest.nodes.some(
      (node) => node.id === nonowningChild && node.parent === null,
    ),
    "Ordinary parent deletion removed or failed to detach its child",
  );
  const selectedInspection = await client.inspect();
  check(
    selectedInspection.entities.some(
      (entity) => entity.id === nonowningChild,
    ) &&
      selectedInspection.controllers?.length === 0 &&
      selectedInspection.renderDiagnostics.length === 0,
    "Aggregate inspection failed for a constraints-only World",
  );

  const world = client.world;
  check(world !== undefined, "Selected World descriptor missing");
  await client.close();
  const reattached = await host.openWorld(selectedWorld.reference);
  check(
    reattached.manifest?.systems.join() === "ipp.constraints",
    "Reattachment lost selected manifest",
  );
  await reattached.close();

  const animationWorld = await host.createWorld({
    symbolicId: "selected-structural-animation",
    selectedSystems: ["ipp.animation"],
  });
  const animation = await host.openWorld(animationWorld.reference);
  const animationManifest = animation.manifest;
  check(animationManifest !== undefined, "Animation manifest missing");
  check(
    animationManifest.operations.includes("animation"),
    "Animation selection missing",
  );
  check(
    animationManifest.components.join() === coreComponents,
    "Animation-only World advertised components",
  );
  for (const field of ["parent", "before"] as const) {
    for (const slot of [
      0xffff_ffff,
      0x1_0000_0000,
      -1,
      0.5,
      NaN,
      Infinity,
      -Infinity,
    ]) {
      const slotRejection = await rejected(
        async () =>
          animation.encodeAnimationClip({
            duration: 1,
            tracks: [
              {
                property: { entityLink: true },
                keys: [
                  {
                    time: 0,
                    value: {
                      kind: "entityPlacement",
                      value: { parent: null, before: null, [field]: slot },
                    },
                  },
                ],
              },
            ],
          }),
        `Invalid ${field} slot ${String(slot)} must reject`,
      );
      check(
        slotRejection.includes("integer out of range"),
        "Structural slot rejection reason",
      );
    }
  }
  const animated = successfulBatch(
    await animation.batch([
      {
        kind: "create",
        alias: 1,
        metadata: { symbolicId: "animated-parent", classes: [] },
      },
      {
        kind: "create",
        alias: 2,
        metadata: { symbolicId: "animated-child", classes: [] },
      },
    ]),
  );
  const animatedParent = aliasId(animated, 1);
  const animatedChild = aliasId(animated, 2);
  const clip = animation.encodeAnimationClip({
    duration: 1,
    tracks: [
      {
        property: { entityLink: true },
        keys: [
          {
            time: 0,
            value: {
              kind: "entityPlacement",
              value: { parent: null, before: null },
            },
            interpolation: { kind: "step" },
          },
          {
            time: 1,
            value: {
              kind: "entityPlacement",
              value: { parent: 0, before: null },
            },
          },
        ],
      },
    ],
  });
  const asset = clientAssetSource(animation.session, 10, 501n);
  await animation.registerAsset(asset, clip.buffer);
  const controller = await animation.createAnimationController({
    speed: 0,
    drivers: [
      {
        source: asset.source,
        track: 0,
        target: animatedChild,
        property: { entityLink: true },
        entityBindings: [animatedParent],
      },
    ],
  });
  await animation.controlAnimationController(controller, { action: "play" });
  await animation.controlAnimationController(controller, { action: "pause" });
  await animation.controlAnimationController(controller, {
    action: "seek",
    time: 1,
  });
  let sampled = false;
  for (let attempt = 0; attempt < 40; attempt++) {
    const page = await animation.inspectPage({ collection: "entities" });
    const child = page.entities.find((entry) => entry.id === animatedChild);
    if (child?.link.parent === animatedParent) {
      sampled = true;
      break;
    }
    await animation.waitForFrame(page.tick);
  }
  check(sampled, "Structural clip-local binding never sampled");
  await animation.controlAnimationController(controller, { action: "stop" });
  const restored = (
    await animation.inspectPage({ collection: "entities" })
  ).entities.find((entry) => entry.id === animatedChild);
  // A structural driver is an absolute writer: stop leaves its placement.
  check(
    restored?.link.parent === animatedParent,
    "Structural animation stop moved its placed link",
  );

  await animation.deleteAnimationController(controller);
  successfulBatch(
    await animation.batch([
      {
        kind: "placeEntity",
        entity: handle(animatedChild),
        placement: { parent: handle(animatedParent), before: null },
      },
    ]),
  );
  const rootClip = animation.encodeAnimationClip({
    duration: 1,
    tracks: [
      {
        property: { entityLink: true },
        keys: [
          {
            time: 0,
            value: {
              kind: "entityPlacement",
              value: { parent: null, before: null },
            },
            interpolation: { kind: "step" },
          },
          {
            time: 1,
            value: {
              kind: "entityPlacement",
              value: { parent: null, before: null },
            },
          },
        ],
      },
    ],
  });
  const rootAsset = clientAssetSource(animation.session, 10, 502n);
  await animation.registerAsset(rootAsset, rootClip.buffer);
  const rootController = await animation.createAnimationController({
    speed: 0,
    drivers: [
      {
        source: rootAsset.source,
        track: 0,
        target: animatedChild,
        property: { entityLink: true },
        entityBindings: [],
      },
    ],
  });
  await animation.controlAnimationController(rootController, {
    action: "play",
  });
  await animation.controlAnimationController(rootController, {
    action: "pause",
  });
  await animation.controlAnimationController(rootController, {
    action: "seek",
    time: 1,
  });
  let rootSampled = false;
  for (let attempt = 0; attempt < 40; attempt++) {
    const page = await animation.inspectPage({ collection: "entities" });
    const child = page.entities.find((entry) => entry.id === animatedChild);
    if (child?.link.parent === null) {
      rootSampled = true;
      break;
    }
    await animation.waitForFrame(page.tick);
  }
  check(rootSampled, "Empty structural binding table never sampled");
  await animation.controlAnimationController(rootController, {
    action: "stop",
  });
  const rootRestored = (
    await animation.inspectPage({ collection: "entities" })
  ).entities.find((entry) => entry.id === animatedChild);
  check(
    rootRestored?.link.parent === null,
    "Root-only stop moved its placed link",
  );
  const animationInspection = await animation.inspect();
  check(
    animationInspection.entities.some(
      (entity) => entity.id === animatedChild,
    ) &&
      animationInspection.controllers?.some(
        (entry) => entry.id === rootController,
      ) &&
      animationInspection.renderDiagnostics.length === 0,
    "Aggregate inspection omitted selected animation controllers",
  );

  return {
    world: world.id.toString(),
    systems: reattached.manifest.systems,
    partial: partial.error.reason,
    animation: controller.toString(),
    rootController: rootController.toString(),
  };
}
