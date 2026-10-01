import type {
  AssetWorldClient,
  ComponentDescriptor,
  WorldPersistenceHostClient,
} from "@ipp/client";
import { successfulBatch, aliasId } from "../camera-fixtures.js";
import { CONSTRAINTS, RENDER, selectSystems } from "../system-selections.js";

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
  const originalWorld = await host.createWorld({
    selectedSystems: selectSystems(CONSTRAINTS),
    symbolicId: "metadata-growth",
    capacityHints: { entities: 1 },
  });
  const client = await host.openWorld(originalWorld.reference);
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
  const bytes = await host.saveWorld(client.session);
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
  const restoredWorld = await loading;
  const restored = await host.openWorld(restoredWorld.root);
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
  await host.destroyWorld(restoredWorld.root);
  await host.destroyWorld(originalWorld.reference);
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
  const originalWorld = await host.createWorld({
    selectedSystems: selectSystems(CONSTRAINTS),
    symbolicId: "authored",
    capacityHints: { entities: 1 },
  });
  const client = await host.openWorld(originalWorld.reference);
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
  const hints = await host.setCapacityHints(client.session, { entities: 4096 });
  check(hints.capacityHints.entities === 4096, "World did not retain hints");

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
  const captured = host.saveWorld(client.session);
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
  const restoredWorld = await host.loadWorld(bytes, {
    symbolicId: "copy",
    capacityHints: { entities: 2 },
  });
  const restored = await host.openWorld(restoredWorld.root);
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
  check(inspection.entities.length === 2, "Restore changed the entity set");
  const base = inspection.entities.find(
    (entity) => entity.metadata.symbolicId === "base",
  );
  check(base, "Symbolic metadata lost");
  const value = base.components.find(
    (entry) => entry.component === component.id,
  )?.fields.value;
  check(value === 7, "Save captured a later write or a stale value");
  await rejected(() => client.inspect(), "Ended World client remained usable");
  await host.destroyWorld(restoredWorld.root);
  await rejected(() => restored.inspect(), "Destroyed World remained usable");
  const original = await host.openWorld(await host.resolveWorld("renamed"));
  check(
    (await original.inspect()).entities.length === 2,
    "Disconnect changed the retained World",
  );
  await original.close();
  await host.destroyWorld(originalWorld.reference);
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
    const world = await leftHost.createWorld({
      selectedSystems: selectSystems(CONSTRAINTS),
      symbolicId: "shared",
    });
    const left = await leftHost.openWorld(world.reference);
    const right = await rightHost.openWorld(
      await rightHost.resolveWorld("shared"),
    );
    check(
      left.world!.id === right.world!.id && left.session !== right.session,
      "Shared World attachment failed",
    );
    const component = scalar(left);
    const offset = component.fields.value!.offset;
    const [a, b] = await Promise.all([
      left.batch([
        {
          kind: "create",
          alias: 0,
          metadata: { symbolicId: "left", classes: [] },
        },
      ]),
      right.batch([
        {
          kind: "create",
          alias: 0,
          metadata: { symbolicId: "right", classes: [] },
        },
      ]),
    ]);
    check(
      aliasId(a, 0) !== aliasId(b, 0),
      "Colliding request IDs routed another session's reply",
    );
    // Entities of a shared World belong to no session: the last write wins
    // whichever session wrote it, and a disconnect removes nothing.
    const leftUi = aliasId(
      successfulBatch(
        await left.batch([
          {
            kind: "create",
            alias: 0,
            metadata: { symbolicId: "left-ui", classes: [] },
          },
          {
            kind: "insertComponent",
            entity: { kind: "alias", alias: 0 },
            component: component.id,
            fields: [{ offset, value: { kind: "f32", value: 5 } }],
          },
        ]),
      ),
      0,
    );
    successfulBatch(
      await right.batch([
        {
          kind: "setField",
          entity: { kind: "symbol", symbol: "left-ui" },
          component: component.id,
          field: { offset, value: { kind: "f32", value: 6 } },
        },
      ]),
    );
    await leftHost.close();
    const survived = await right.inspect();
    check(
      survived.entities.length === 3,
      "Disconnect removed entities of a shared World",
    );
    check(
      survived.entities
        .find((entity) => entity.id === leftUi)
        ?.components.find((entry) => entry.component === component.id)?.fields
        .value === 6,
      "Another session's write did not win",
    );
    await rightHost.destroyWorld(world.reference);
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
    const world = await leftHost.createWorld({
      selectedSystems: selectSystems(RENDER, CONSTRAINTS),
      symbolicId: "shared-assets",
    });
    const left = await leftHost.openWorld(world.reference);
    const right = await rightHost.openWorld(
      await rightHost.resolveWorld("shared-assets"),
    );
    const a = new AnimationFixture(left, contract, async () => {});
    const b = new AnimationFixture(right, contract, async () => {});
    const targetA = await a.create("left-target", { Scalar: { value: 3 } });
    const targetB = await b.create("right-target", { Scalar: { value: 4 } });
    const sourceA = await a.upload(a.curve("Scalar", "value", 0, 0), 501n);
    // At x=.4375 the left curve has changed by 6 and the right by 56; each
    // controller adds that change to its target.
    const sourceB = await b.upload(b.curve("Scalar", "value", 0, 100), 501n);
    const controllerA = await a.controller([
      a.driver(targetA, sourceA, 0, "Scalar", ["value"]),
    ]);
    const controllerB = await b.controller([
      b.driver(targetB, sourceB, 0, "Scalar", ["value"]),
    ]);
    const sampledA = await a.seekPaused(controllerA, 0.4375);
    const sampledB = await b.seekPaused(controllerB, 0.4375);
    check(
      Math.abs(a.value(sampledA, targetA, "Scalar", "value") - 9) < 1e-4,
      "Left source was replaced",
    );
    check(
      Math.abs(b.value(sampledB, targetB, "Scalar", "value") - 60) < 1e-4,
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
    const bytes = await rightHost.saveWorld(right.session);
    await right.close();
    const restoredWorld = await rightHost.loadWorld(bytes, {
      symbolicId: "shared-assets-copy",
    });
    const restored = await rightHost.openWorld(restoredWorld.root);
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
