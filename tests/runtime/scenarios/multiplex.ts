import { sameOutputReference } from "../../../packages/ipp-client/src/references.js";
import type {
  Client,
  Command,
  HostClientBase,
  PickingWorldClient,
  ViewQueryTarget,
} from "@ipp/client";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
  pageCommands,
} from "../../fixtures/commands.js";
import {
  ATTACHMENTS,
  CAMERA,
  selectSystems,
} from "../../fixtures/system-selections.js";
import { check } from "../../harness/page/checks.js";

async function rejects(action: () => Promise<unknown>, message: string) {
  try {
    await action();
  } catch (error) {
    check(error instanceof Error, message);
    return error.message;
  }
  throw new Error(message);
}

async function referenceFailure(
  client: Client,
  commands: Command[],
  failedOperation: number,
) {
  const outcome = await client.batch(commands);
  check(
    !outcome.ok &&
      outcome.error.operation === failedOperation &&
      outcome.aliases.length === 1,
    "Stale runtime reference discarded the correlated applied prefix",
  );
  check(
    (await client.inspect()).entities.some(
      (entity) => entity.id === outcome.aliases[0]!.id,
    ),
    "Stale runtime reference rolled back the applied identity",
  );
  return outcome.aliases[0]!.id;
}

export async function multiplexSessions(host: HostClientBase<Client>) {
  try {
    await host.presentation.surface();
    throw new Error("Headless Host claimed a drawing surface");
  } catch (error) {
    check(
      error instanceof Error &&
        "reason" in error &&
        error.reason === "unsupported",
      "Headless presentation must reject explicitly without closing Host",
    );
  }
  const parentWorld = await host.createWorld({
    selectedSystems: selectSystems(ATTACHMENTS, CAMERA),
    symbolicId: "mux-parent",
  });
  const childWorld = await host.createWorld({
    selectedSystems: selectSystems(CAMERA),
    symbolicId: "mux-child",
  });
  const emptyWorld = await host.createWorld({
    symbolicId: "mux-empty",
    selectedSystems: [],
  });
  check(
    [...host.sessions].length === 0,
    "Creation implicitly opened a session",
  );
  await rejects(
    () =>
      host.openWorld({
        ...childWorld.reference,
        incarnation: childWorld.reference.incarnation + 1n,
      }),
    "Forged World lifetime opened",
  );
  check(
    (await host.listWorlds()).length === 3,
    "Failed open destroyed the observable creation",
  );
  const parent = await host.openWorld(parentWorld.reference);
  const child = await host.openWorld(childWorld.reference);
  const peer = await host.openWorld(childWorld.reference);
  const empty = await host.openWorld(emptyWorld.reference);
  check(
    child.session !== peer.session && [...host.sessions].length === 4,
    "Sessions were conflated",
  );
  check(
    child.worldReference?.incarnation === childWorld.reference.incarnation,
    "Ready session omitted exact World identity",
  );
  check(
    empty.manifest?.systems.length === 0,
    "Readiness described compiled rather than selected Systems",
  );
  check(
    (await empty.inspect()).entities.length === 0,
    "Empty World aggregate inspection failed",
  );
  await rejects(
    () => empty.inspectPage({ collection: "controllers" }),
    "Explicit unsupported inspection succeeded",
  );

  const camera = aliasId(
    await child.batch([
      createEntity(1, "mux-camera"),
      insertComponent(child, "Camera", { kind: "alias", alias: 1 }),
      insertComponent(
        child,
        "Transform",
        { kind: "alias", alias: 1 },
        { z: 5 },
      ),
    ]),
    1,
  );
  const output = await host.bindOutput(childWorld.reference, camera, "camera");
  check(
    sameOutputReference(await host.resolveOutput(output), output),
    "Exact output resolution changed identity",
  );
  const initialBinding = await host.setRootOutput(output, {
    width: 640,
    height: 480,
    devicePixelRatio: 1,
  });
  const binding = await host.setRootOutput(output, initialBinding.viewport);
  check(
    binding.generation.host === initialBinding.generation.host &&
      binding.generation.serial > initialBinding.generation.serial,
    "Equal root rebind reused its generation",
  );
  await host.clearRootOutput(initialBinding);
  check(
    (await host.getRootOutputBinding(childWorld.reference))?.generation
      .serial === binding.generation.serial,
    "Stale root cleanup erased a replacement binding",
  );
  const queries = child as PickingWorldClient;
  const view: ViewQueryTarget = {
    kind: "root",
    output,
    expectedViewport: { width: 640, height: 480, devicePixelRatio: 1 },
  };
  const picked = await queries.query({
    type: "GeometryPickQuery",
    view,
    x: 0.5,
    y: 0.5,
  });
  check(
    picked.ok &&
      picked.hit === null &&
      sameOutputReference(picked.view.output, output) &&
      picked.view.output.world.incarnation === childWorld.reference.incarnation,
    "Completed-view pick lost exact output identity",
  );
  const projected = await queries.query({
    type: "CameraProjectQuery",
    view,
    x: 0.5,
    y: 0.5,
    plane: { point: [0, 0, 0], normal: [0, 0, 1] },
  });
  check(
    projected.ok &&
      projected.position?.every((value) => Math.abs(value) < 1e-5),
    "Explicit completed-view projection failed",
  );
  const mismatch = await queries.query({
    type: "GeometryPickQuery",
    view: {
      ...view,
      expectedViewport: { width: 1, height: 1, devicePixelRatio: 1 },
    },
    x: 0.5,
    y: 0.5,
  });
  check(
    !mismatch.ok && mismatch.error === "InvalidViewport",
    "Query ignored exact root viewport",
  );
  for (const query of [
    { type: "GeometryPickQuery", x: 0.5, y: 0.5 },
    {
      type: "CameraProjectQuery",
      x: 0.5,
      y: 0.5,
      plane: { point: [0, 0, 0], normal: [0, 0, 1] },
    },
  ]) {
    const error = await queries
      .query(query as unknown as Parameters<typeof queries.query>[0])
      .then(
        () => undefined,
        (error: unknown) => error,
      );
    check(
      error instanceof Error &&
        "code" in error &&
        error.code === "IPP_REQUEST_NOT_SENT" &&
        /view target required/.test(error.message),
      `A ${query.type} without a view was not refused before sending`,
    );
  }
  await rejects(
    () =>
      (empty as PickingWorldClient).query({
        type: "GeometryPickQuery",
        view,
        x: 0.5,
        y: 0.5,
      }),
    "Unsupported selected-World geometry query admitted",
  );
  await rejects(
    () =>
      (empty as PickingWorldClient).query({
        type: "CameraProjectQuery",
        view,
        x: 0.5,
        y: 0.5,
        plane: { point: [0, 0, 0], normal: [0, 0, 1] },
      }),
    "Unsupported selected-World projection admitted",
  );
  const foreign = await (parent as PickingWorldClient).query({
    type: "GeometryPickQuery",
    view,
    x: 0.5,
    y: 0.5,
  });
  check(
    !foreign.ok && foreign.error === "InvalidEntity",
    "View query crossed its authoring World fence",
  );
  await host.clearRootOutput(binding);
  const cleared = await queries.query({
    type: "GeometryPickQuery",
    view,
    x: 0.5,
    y: 0.5,
  });
  check(
    !cleared.ok && cleared.error === "InvalidEntity",
    "Cleared root fell back to headless active camera",
  );
  const history = await queries.query({
    type: "CameraProjectQuery",
    view: {
      kind: "publication",
      output,
      publication: { host: picked.view.publication.host, revision: 0n },
      viewport: view.expectedViewport,
    },
    x: 0.5,
    y: 0.5,
    plane: { point: [0, 0, 0], normal: [0, 0, 1] },
  });
  check(
    !history.ok && history.error === "InvalidEntity",
    "Unavailable explicit history fell back to latest publication",
  );

  // An open batch's first page has left, but nothing applies before its final page.
  const open = child.openBatch();
  open.write(
    Array.from({ length: pageCommands(child) + 1 }, (_, index) =>
      createEntity(index + 2, index ? `open-child-${index}` : "open-child"),
    ),
  );
  const parentOutcome = await parent.batch([
    createEntity(1, "parent-progress"),
  ]);
  check(
    parentOutcome.ok,
    "An open child batch blocked another World on the same connection",
  );
  check(
    (await empty.inspect()).tick > 0n,
    "An open child batch blocked an independent empty World",
  );
  check(
    !(await peer.inspect()).entities.some((entity) =>
      entity.metadata.symbolicId?.startsWith("open-child"),
    ),
    "A peer observed a page of an unfinished batch",
  );
  successfulBatch(await open.finish());
  check(
    (await peer.inspect()).entities.some(
      (entity) => entity.metadata.symbolicId === "open-child",
    ),
    "Peer session did not observe the completed batch",
  );

  const partial = await parent.batch([
    createEntity(4, "partial-kept"),
    { kind: "delete", entity: { kind: "handle", id: 0xffff_ffff_ffff_ffffn } },
  ]);
  check(
    !partial.ok &&
      partial.error.operation === 1 &&
      partial.aliases.length === 1,
    "Partial effects lost their exact session correlation",
  );
  check(
    (await parent.inspect()).entities.some(
      (entity) => entity.id === partial.aliases[0]!.id,
    ),
    "Applied partial entity disappeared",
  );

  const attachment = parent.components.WorldAttachment;
  check(
    attachment?.fields.child &&
      attachment.fields.output &&
      attachment.fields.mode,
    "Runtime reference schema missing",
  );
  await referenceFailure(
    parent,
    [
      createEntity(6, "stale-world-prefix"),
      {
        kind: "insertComponent",
        entity: { kind: "alias", alias: 6 },
        component: attachment.id,
        fields: [
          {
            offset: attachment.fields.child.offset,
            value: {
              kind: "world",
              value: {
                ...childWorld.reference,
                incarnation: childWorld.reference.incarnation + 1n,
              },
            },
          },
        ],
      },
    ],
    1,
  );
  const anchor = aliasId(
    await parent.batch([
      createEntity(5, "child-anchor"),
      {
        kind: "insertComponent",
        entity: { kind: "alias", alias: 5 },
        component: attachment.id,
        fields: [
          {
            offset: attachment.fields.child.offset,
            value: { kind: "world", value: childWorld.reference },
          },
          {
            offset: attachment.fields.output.offset,
            value: { kind: "output", value: null },
          },
          {
            offset: attachment.fields.mode.offset,
            value: { kind: "u32", value: 0 },
          },
        ],
      },
    ]),
    5,
  );
  const attached = (await parent.inspect()).entities
    .find((entity) => entity.id === anchor)
    ?.components.find((component) => component.component === attachment.id);
  const childValue = attached?.fields.child;
  check(
    typeof childValue === "object" &&
      childValue !== null &&
      "incarnation" in childValue &&
      childValue.incarnation === childWorld.reference.incarnation,
    "World reference did not roundtrip through ordinary component inspection",
  );
  successfulBatch(
    await parent.batch([
      {
        kind: "removeComponent",
        entity: { kind: "handle", id: anchor },
        component: attachment.id,
      },
    ]),
  );

  successfulBatch(
    await child.batch([
      {
        kind: "removeComponent",
        entity: { kind: "handle", id: camera },
        component: child.components.Camera!.id,
      },
      insertComponent(child, "Camera", { kind: "handle", id: camera }),
    ]),
  );
  await rejects(
    () => host.resolveOutput(output),
    "Stale output silently rebound to replacement",
  );
  await rejects(
    () =>
      host.setRootOutput(output, {
        width: 640,
        height: 480,
        devicePixelRatio: 1,
      }),
    "Stale output selected presentation",
  );
  const rebound = await host.bindOutput(childWorld.reference, camera, "camera");
  check(
    rebound.incarnation !== output.incarnation,
    "Explicit producer replacement reused its incarnation",
  );
  const outputPrefix = await referenceFailure(
    parent,
    [
      createEntity(7, "stale-output-prefix"),
      insertComponent(
        parent,
        "Transform",
        { kind: "alias", alias: 7 },
        { x: 9 },
      ),
      {
        kind: "insertComponent",
        entity: { kind: "alias", alias: 7 },
        component: attachment.id,
        fields: [
          {
            offset: attachment.fields.output.offset,
            value: { kind: "output", value: output },
          },
        ],
      },
    ],
    2,
  );
  check(
    (await parent.inspect()).entities
      .find((entity) => entity.id === outputPrefix)
      ?.components.find(
        (component) => component.component === parent.components.Transform!.id,
      )?.fields.x === 9,
    "Stale output lost the preceding successful component mutation",
  );

  const parentPeer = await host.openWorld(parentWorld.reference);
  // The output reference travels in a later page than its first page and is
  // validated when the whole batch applies, not when its page arrives.
  const queued = parentPeer.openBatch();
  queued.write([
    ...Array.from({ length: pageCommands(parentPeer) - 1 }, (_, index) =>
      createEntity(100 + index, `queued-filler-${index}`),
    ),
    createEntity(8, "queued-output-prefix"),
    {
      kind: "insertComponent",
      entity: { kind: "alias", alias: 8 },
      component: attachment.id,
      fields: [
        {
          offset: attachment.fields.output.offset,
          value: { kind: "output", value: rebound },
        },
      ],
    },
  ]);
  successfulBatch(
    await child.batch([
      {
        kind: "removeComponent",
        entity: { kind: "handle", id: camera },
        component: child.components.Camera!.id,
      },
    ]),
  );
  const queuedOutcome = await queued.finish();
  check(
    !queuedOutcome.ok &&
      queuedOutcome.error.operation === pageCommands(parentPeer) &&
      queuedOutcome.aliases.length === pageCommands(parentPeer),
    "Queued output was not revalidated at ordered operation execution",
  );
  await parentPeer.close();
  await child.close();
  check(
    [...host.sessions].length === 3 &&
      (await peer.inspect()).entities.length >= 2,
    "Closing one session invalidated its peer or World",
  );
  await rejects(() => child.inspect(), "Closed session remained usable");
  await host.destroyWorld(childWorld.reference);
  await rejects(
    () => peer.inspect(),
    "Destroyed World session remained usable",
  );
  await rejects(
    () => host.openWorld(childWorld.reference),
    "Destroyed World reference reopened",
  );
  await rejects(
    () => host.resolveOutput(rebound),
    "Destroyed producer output resolved",
  );
  check(
    (await parent.inspect()).entities.length >= 3,
    "Child destruction damaged parent session",
  );
  await parent.close();
  await empty.close();
  await host.destroyWorld(parentWorld.reference);
  await host.destroyWorld(emptyWorld.reference);
  check(
    (await host.listWorlds()).length === 0,
    "Explicit teardown leaked a World",
  );
  return {
    sessions: 4,
    openBatchApplied: true,
    outputIncarnation: output.incarnation.toString(),
    replacementIncarnation: rebound.incarnation.toString(),
    partialAliases: partial.aliases.length,
    staleReferencePrefixes: 3,
  };
}

/**
 * More attached Worlds than the Host admits requests on one connection mount at
 * once, as concurrent `AttachedWorld`s do: each creates and opens its World,
 * authors a camera, binds and resolves its output and attaches the World under
 * a shared parent,
 * then every session floods the connection with batches. Requests beyond the
 * Host's admission window wait in the connection's flow control instead of
 * failing; every mount and batch must succeed on the one connection.
 */
export async function concurrentAttachedWorlds(
  host: HostClientBase<Client>,
  count = 80,
  batchesPerWorld = 4,
) {
  const parentWorld = await host.createWorld({
    selectedSystems: selectSystems(ATTACHMENTS, CAMERA),
    symbolicId: "credit-parent",
  });
  const parent = await host.openWorld(parentWorld.reference);
  const attachment = parent.components.WorldAttachment;
  const child = attachment?.fields.child;
  const outputField = attachment?.fields.output;
  const mode = attachment?.fields.mode;
  check(
    attachment && child && outputField && mode,
    "Runtime reference schema missing",
  );
  const children = await Promise.all(
    Array.from({ length: count }, async (_, index) => {
      const world = await host.createWorld({
        selectedSystems: selectSystems(CAMERA),
        symbolicId: `credit-child-${index}`,
      });
      const session = await host.openWorld(world.reference);
      const camera = aliasId(
        await session.batch([
          createEntity(1, `credit-camera-${index}`),
          insertComponent(session, "Camera", { kind: "alias", alias: 1 }),
          insertComponent(
            session,
            "Transform",
            { kind: "alias", alias: 1 },
            { z: 5 },
          ),
        ]),
        1,
      );
      const output = await host.bindOutput(world.reference, camera, "camera");
      check(
        sameOutputReference(await host.resolveOutput(output), output),
        "A concurrently bound output changed identity",
      );
      const anchor = aliasId(
        await parent.batch([
          createEntity(1, `credit-anchor-${index}`),
          {
            kind: "insertComponent",
            entity: { kind: "alias", alias: 1 },
            component: attachment.id,
            fields: [
              {
                offset: child.offset,
                value: { kind: "world", value: world.reference },
              },
              // A spatial attachment: this headless target has no Surfaces
              // to present the bound output through.
              {
                offset: outputField.offset,
                value: { kind: "output", value: null },
              },
              {
                offset: mode.offset,
                value: { kind: "u32", value: 0 },
              },
            ],
          },
        ]),
        1,
      );
      return { world, session, anchor };
    }),
  );
  check(
    new Set(children.map(({ session }) => session.session)).size === count &&
      [...host.sessions].length === count + 1,
    "Concurrently mounted Worlds did not each receive their own session",
  );

  // Far more requests than the Host admits at once, from every session.
  const flood = await Promise.all(
    children.flatMap(({ session }, index) =>
      Array.from({ length: batchesPerWorld }, (_, batch) =>
        session.batch([
          createEntity(1, `credit-flood-${index}-${batch}`),
          insertComponent(
            session,
            "Transform",
            { kind: "alias", alias: 1 },
            { x: batch },
          ),
        ]),
      ),
    ),
  );
  check(
    flood.every((outcome) => outcome.ok),
    "A request waiting for connection credit failed",
  );
  const anchors = new Set(
    (await parent.inspect()).entities
      .filter((entity) =>
        entity.components.some(
          (component) => component.component === attachment.id,
        ),
      )
      .map((entity) => entity.id),
  );
  check(
    children.every(({ anchor }) => anchors.has(anchor)),
    "A concurrently mounted World was not attached",
  );
  for (const { session } of children.slice(0, 3))
    check(
      (await session.inspect()).entities.length === 1 + batchesPerWorld,
      "A flooded session lost or duplicated batches",
    );

  await Promise.all(
    children.map(async ({ session, world }) => {
      await session.close();
      await host.destroyWorld(world.reference);
    }),
  );
  await parent.close();
  await host.destroyWorld(parentWorld.reference);
  check(
    (await host.listWorlds()).length === 0,
    "Concurrent teardown leaked a World",
  );
  return {
    attachedWorlds: count,
    requests: count * (6 + batchesPerWorld),
  };
}
