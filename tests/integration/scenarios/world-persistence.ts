import type {
  AssetWorldClient,
  ComponentDescriptor,
  WorldPersistenceHostClient,
} from "@ipp/client";
import { successfulBatch, aliasId } from "../camera-fixtures.js";

type PersistenceHost = WorldPersistenceHostClient<AssetWorldClient>;

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

async function rejected(
  action: () => Promise<unknown>,
  message: string,
): Promise<void> {
  let error: unknown;
  try {
    await action();
  } catch (caught) {
    error = caught;
  }
  check(error instanceof Error, message);
}

function scalar(client: AssetWorldClient): ComponentDescriptor {
  const scalar = client.components.Scalar;
  check(scalar?.fields.value, "Scalar contract missing");
  return scalar;
}

/** Grow beyond the former metadata estimate through ordinary bounded batches. */
export async function worldMetadataGrowth(host: PersistenceHost) {
  const client = await host.createWorld({
    symbolicId: "metadata-growth",
    capacityHints: { entities: 1 },
  });
  const classes = Array.from(
    { length: 48 },
    (_, index) => `class-${index}-${"x".repeat(96)}`,
  );
  const entityCount = 192;
  // The old estimator charged at least 128 bytes per class before any text.
  check(
    entityCount * classes.length * 128 > 1024 * 1024,
    "Fixture is too small",
  );
  for (let start = 0; start < entityCount; start += 4) {
    successfulBatch(
      await client.batch(
        Array.from({ length: 4 }, (_, alias) => ({
          kind: "create" as const,
          alias,
          metadata: { symbolicId: `entity-${start + alias}`, classes },
        })),
      ),
    );
  }
  const before = await client.inspect();
  check(before.entities.length === entityCount, "World growth lost entities");
  const bytes = await host.saveWorld();
  check(
    bytes.length > 8 * 65_536,
    "Fixture must span multiple transfer windows",
  );
  await client.close();
  const abort = new AbortController();
  await rejected(() => {
    const loading = host.loadWorld(bytes, { signal: abort.signal });
    abort.abort();
    return loading;
  }, "Cancelled load must reject");
  const corrupt = bytes.slice();
  corrupt[corrupt.length - 1]! ^= 1;
  await rejected(
    () => host.loadWorld(corrupt, { symbolicId: "invalid-copy" }),
    "Corrupt multi-window load must reject",
  );
  check(
    (await host.listWorlds()).length === 1,
    "Failed load published a World",
  );
  const input = bytes.slice();
  const loading = host.loadWorld(input, { symbolicId: "metadata-copy" });
  input.fill(0);
  const restored = await loading;
  const after = await restored.inspect();
  const expectedClasses = [...classes].sort().join(",");
  check(
    after.entities.length === entityCount,
    "Restore lost grown World entities",
  );
  for (const entity of after.entities) {
    check(
      entity.metadata.classes.join(",") === expectedClasses,
      "Restore lost classes",
    );
    check(
      before.entities.some(
        (original) =>
          original.metadata.symbolicId === entity.metadata.symbolicId,
      ),
      "Restore lost an identity",
    );
  }
  successfulBatch(
    await restored.batch([
      {
        kind: "create",
        alias: 0,
        metadata: { symbolicId: "after-restore", classes: [] },
      },
    ]),
  );
  check(
    (await restored.inspect()).entities.length === entityCount + 1,
    "Restored World could not grow further",
  );
  await host.destroyWorld("metadata-copy");
  await host.destroyWorld("metadata-growth");
  return {
    entities: entityCount,
    classesPerEntity: classes.length,
    savedBytes: bytes.length,
  };
}

/** Same scenario through native WebSocket and browser worker/WASM. No wire or clock control. */
export async function namedWorldPersistence(
  host: PersistenceHost,
): Promise<{ bytes: number; persistentId: string }> {
  check(
    (await host.listWorlds()).length === 0,
    "Connect must not create a World",
  );
  const client = await host.createWorld({
    symbolicId: "authored",
    capacityHints: { entities: 1 },
  });
  const world = client.world!;
  const component = scalar(client);
  const offset = component.fields.value!.offset;
  const created = await client.batch([
    {
      kind: "create",
      alias: 0,
      metadata: { symbolicId: "base", classes: ["saved"] },
    },
    {
      kind: "insertComponent",
      entity: { kind: "alias", alias: 0 },
      component: component.id,
      fields: [{ offset, value: { kind: "f32", value: 2 } }],
    },
    { kind: "create", alias: 1, metadata: { symbolicId: null, classes: [] } },
  ]);
  const entity = aliasId(created, 0);
  check(
    (await client.inspect()).entities.length === 2,
    "Entity reservation became a ceiling",
  );
  const renamed = await host.renameWorld(world.id, "renamed");
  check(
    renamed.id === world.id && renamed.persistentId === world.persistentId,
    "Rename changed identity",
  );
  const hints = await host.setCapacityHints({ entities: 4096 });
  check(hints.capacityHints.entities === 4096, "World did not retain hints");

  const declaration = successfulBatch(
    await client.batch([
      { kind: "createStateOverlayOwner", alias: 0 },
      {
        kind: "attachEntityOverlayBinding",
        owner: { kind: "alias", alias: 0 },
        alias: 1,
        symbolicId: "base",
        mode: "bound",
      },
      {
        kind: "attachComponentStateOverlay",
        owner: { kind: "alias", alias: 0 },
        binding: { kind: "alias", alias: 1 },
        alias: 2,
        component: component.id,
        mode: "bound",
        fields: [{ offset, value: { kind: "f32", value: 99 } }],
      },
      {
        kind: "attachEntityOverlayBinding",
        owner: { kind: "alias", alias: 0 },
        alias: 3,
        symbolicId: "temporary",
        mode: "owned",
      },
    ]),
  );
  check(declaration.stateOverlays.length === 4, "Overlay setup failed");

  // A save is ordered between its preceding and following authored writes, even
  // when all three operations are queued without awaiting intermediate results.
  const before = client.batch([
    {
      kind: "setField",
      entity: { kind: "handle", id: entity },
      component: component.id,
      field: { offset, value: { kind: "f32", value: 7 } },
    },
  ]);
  const captured = host.saveWorld();
  const after = client.batch([
    {
      kind: "setField",
      entity: { kind: "handle", id: entity },
      component: component.id,
      field: { offset, value: { kind: "f32", value: 11 } },
    },
  ]);
  successfulBatch(await before);
  const bytes = await captured;
  successfulBatch(await after);
  check(bytes.length > 32, "Save returned no World file");
  await client.close();
  check(
    (await host.listWorlds()).length === 1,
    "Disconnect deleted a retained World",
  );
  await rejected(
    () => host.loadWorld(bytes),
    "Name collision must reject load",
  );
  check(
    (await host.listWorlds()).length === 1,
    "Failed load published a World",
  );
  const corrupt = bytes.slice();
  corrupt[corrupt.length - 1]! ^= 1;
  await rejected(
    () => host.loadWorld(corrupt, { symbolicId: "corrupt" }),
    "Corrupt file must reject",
  );
  const restored = await host.loadWorld(bytes, {
    symbolicId: "copy",
    capacityHints: { entities: 2 },
  });
  check(restored.session !== client.session, "Load reused a World session");
  check(
    restored.world!.persistentId === world.persistentId,
    "Durable identity was lost",
  );
  check(
    restored.world!.capacityHints.entities === 2,
    "Load override was ignored",
  );
  const inspection = await restored.inspect();
  check(inspection.entities.length === 2, "Owned UI entity leaked into file");
  const base = inspection.entities.find(
    (entity) => entity.metadata.symbolicId === "base",
  );
  check(base, "Symbolic metadata lost");
  const value = base.base.find((entry) => entry.component === component.id)
    ?.fields.value;
  check(value === 7, "Save captured an overlay, later write or stale base");
  await rejected(() => client.inspect(), "Ended World client remained usable");
  await host.destroyWorld(restored.world!.id);
  await rejected(() => restored.inspect(), "Destroyed World remained usable");
  const original = await host.attachWorld("renamed");
  check(
    (await original.inspect()).entities.length === 2,
    "Disconnect failed to clean owned entity",
  );
  await original.close();
  await host.destroyWorld("renamed");
  check(
    (await host.listWorlds()).length === 0,
    "World discovery retained destroyed Worlds",
  );
  return { bytes: bytes.length, persistentId: world.persistentId.toString(16) };
}

export async function sharedWorldSessions(
  connect: () => Promise<PersistenceHost>,
) {
  const leftHost = await connect();
  const rightHost = await connect();
  try {
    const left = await leftHost.createWorld({ symbolicId: "shared" });
    const right = await rightHost.attachWorld("shared");
    check(
      left.world!.id === right.world!.id && left.session !== right.session,
      "Shared World attachment failed",
    );
    const component = scalar(left);
    const offset = component.fields.value!.offset;
    const [a, b] = await Promise.all([
      left.batch(
        [
          {
            kind: "create",
            alias: 0,
            metadata: { symbolicId: "left", classes: [] },
          },
        ],
        1n,
      ),
      right.batch(
        [
          {
            kind: "create",
            alias: 0,
            metadata: { symbolicId: "right", classes: [] },
          },
        ],
        1n,
      ),
    ]);
    check(
      aliasId(a, 0) !== aliasId(b, 0),
      "Colliding request IDs routed another session's reply",
    );
    const owner = successfulBatch(
      await left.batch([
        { kind: "createStateOverlayOwner", alias: 0 },
        {
          kind: "attachEntityOverlayBinding",
          owner: { kind: "alias", alias: 0 },
          alias: 1,
          symbolicId: "left-ui",
          mode: "owned",
        },
        {
          kind: "attachComponentStateOverlay",
          owner: { kind: "alias", alias: 0 },
          binding: { kind: "alias", alias: 1 },
          alias: 2,
          component: component.id,
          mode: "owned",
          fields: [{ offset, value: { kind: "f32", value: 5 } }],
        },
      ]),
    ).stateOverlays[0]!.id;
    await rejected(
      () =>
        right.batch([
          {
            kind: "releaseStateOverlayOwner",
            owner: { kind: "handle", id: owner },
          },
        ]),
      "Another session released the owner",
    );
    check(
      (await right.inspect()).entities.length === 3,
      "Foreign owner rejection changed World state",
    );
    await leftHost.close();
    check(
      (await right.inspect()).entities.length === 2,
      "Disconnect left owned UI entities",
    );
    await rightHost.destroyWorld("shared");
    check(
      (await rightHost.listWorlds()).length === 0,
      "Shared destruction did not detach sessions",
    );
  } finally {
    await leftHost.close();
    await rightHost.close();
  }
}

/** Two producers can use the same local asset ID inside one shared World. */
export async function sharedWorldAssetSources(
  connect: () => Promise<
    WorldPersistenceHostClient<import("@ipp/client").AnimationWorldClient>
  >,
  contract: import("../animation-fixtures.js").AnimationContract,
): Promise<void> {
  const { AnimationFixture } = await import("../animation-fixtures.js");
  const leftHost = await connect();
  const rightHost = await connect();
  try {
    const left = await leftHost.createWorld({ symbolicId: "shared-assets" });
    const right = await rightHost.attachWorld("shared-assets");
    const a = new AnimationFixture(left, contract, async () => {});
    const b = new AnimationFixture(right, contract, async () => {});
    const targetA = await a.create("left-target", { Scalar: { value: 3 } });
    const targetB = await b.create("right-target", { Scalar: { value: 4 } });
    const sourceA = await a.upload(a.curve("Scalar", "value", 0, 0), 501n);
    const sourceB = await b.upload(b.curve("Scalar", "value", 100, 100), 501n);
    const controllerA = await a.controller([
      a.driver(targetA, sourceA, 0, "Scalar", ["value"]),
    ]);
    const controllerB = await b.controller([
      b.driver(targetB, sourceB, 0, "Scalar", ["value"]),
    ]);
    const sampledA = await a.seekPaused(controllerA, 0.4375);
    const sampledB = await b.seekPaused(controllerB, 0.4375);
    check(
      a.value(sampledA, targetA, "Scalar", "value") === 6,
      "Left source was replaced",
    );
    check(
      b.value(sampledB, targetB, "Scalar", "value") === 106,
      "Right source aliased another client",
    );
    check(
      sampledB.resources.length === 2,
      "Client source namespaces did not retain two assets",
    );
    check(
      sampledB.resources[0]!.id !== sampledB.resources[1]!.id,
      "Client sources share a runtime identity",
    );
    await leftHost.close();
    const bytes = await rightHost.saveWorld();
    await rightHost.detachWorld();
    const restored = await rightHost.loadWorld(bytes, {
      symbolicId: "shared-assets-copy",
    });
    const state = await restored.inspect();
    check(state.entities.length === 2, "Shared authored entities were lost");
    check(state.controllers?.length === 2, "Controller descriptions were lost");
    const drivers = state.controllers.flatMap(
      (controller) => controller.description.drivers,
    );
    check(
      drivers.length === 2 &&
        drivers.every(
          (driver) => driver.source === sourceA || driver.source === sourceB,
        ) &&
        drivers.some((driver) => driver.source === sourceA) &&
        drivers.some((driver) => driver.source === sourceB),
      "Save rewrote a client source",
    );
    const restoredTargetA = state.entities.find(
      (entity) => entity.metadata.symbolicId === "left-target",
    );
    const restoredTargetB = state.entities.find(
      (entity) => entity.metadata.symbolicId === "right-target",
    );
    check(restoredTargetA && restoredTargetB, "Saved targets were lost");
    check(
      drivers.every(
        (driver) =>
          driver.target ===
          (driver.source === sourceA ? restoredTargetA.id : restoredTargetB.id),
      ),
      "Save failed to remap a controller target",
    );
    for (const controller of state.controllers) {
      check(
        controller.state === "paused" && controller.time === 0.4375,
        "Saved clock was lost",
      );
    }
    check(
      !new TextDecoder().decode(bytes).includes("bundle://"),
      "Save bundled assets",
    );
  } finally {
    await leftHost.close();
    await rightHost.close();
  }
}
