import type {
  Client,
  Command,
  Inspection,
  WorldPersistenceHostClient,
} from "@ipp/client";
import {
  aliasId,
  componentFields,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";
import { check } from "../animation-fixtures.js";
import { CONSTRAINTS, RENDER, selectSystems } from "../system-selections.js";

export async function assetRejectionRecovery(
  host: WorldPersistenceHostClient<Client>,
  record: (kind: string, value: unknown) => Promise<void>,
) {
  const leftWorld = await host.createWorld({
    selectedSystems: selectSystems(RENDER, CONSTRAINTS),
    temporary: true,
  });
  const rightWorld = await host.createWorld({
    selectedSystems: selectSystems(RENDER, CONSTRAINTS),
    temporary: true,
  });
  const left = await host.openWorld(leftWorld.reference);
  const right = await host.openWorld(rightWorld.reference);
  const shared = "ipp://mesh/cube?width=1&height=1&length=1";
  const create = async (client: Client) =>
    aliasId(
      await client.batch([
        createEntity(1, "asset-rejection"),
        insertComponent(
          client,
          "Scalar",
          { kind: "alias", alias: 1 },
          { value: 1 },
        ),
        insertComponent(
          client,
          "MeshInstance",
          { kind: "alias", alias: 1 },
          { source: shared },
        ),
      ]),
      1,
    );
  const write = (
    client: Client,
    entity: bigint,
    component: string,
    values: Record<string, string | number>,
  ): Command[] =>
    componentFields(client, component, values).map((field) => ({
      kind: "setField",
      entity: { kind: "handle", id: entity },
      component: client.components[component]!.id,
      field,
    }));
  const field = (
    client: Client,
    state: Inspection,
    entity: bigint,
    name: string,
    property: string,
  ) =>
    state.entities
      .find((item) => item.id === entity)
      ?.components.find(
        (component) => component.component === client.components[name]!.id,
      )?.fields[property];
  const ready = async (client: Client) => {
    for (let attempt = 0; attempt < 120; attempt++) {
      const state = await client.inspect();
      const resource = state.resources.find(
        (resource) => resource.source === shared,
      );
      if (resource?.status === "loaded") return resource;
      await client.waitForFrame(state.tick);
    }
    throw new Error("Shared resource did not become ready");
  };

  try {
    const leftEntity = await create(left);
    const rightEntity = await create(right);
    const resource = await ready(left);
    check(
      (await ready(right)).id === resource.id,
      "Worlds failed to share the resource identity",
    );
    for (const source of [
      `producer://${leftWorld.id}/1/ordinary-motion-A`,
      `producer://${leftWorld.id}/2/42`,
      `producer://${leftWorld.id}/1/0`,
    ]) {
      const operations = [
        ...write(left, leftEntity, "Scalar", { value: 7 }),
        ...write(left, leftEntity, "MeshInstance", { source }),
        ...write(left, leftEntity, "Scalar", { value: 99 }),
      ];
      await record("asset-rejection.request", {
        world: leftWorld.id,
        operations,
      });
      const rejected = await left.batch(operations);
      await record("asset-rejection.outcome", { source, rejected });
      check(!rejected.ok, "Malformed source was accepted");
      check(
        rejected.error.scope === "operation" && rejected.error.operation === 1,
        "Rejection lost the failing operation index",
      );
      check(
        rejected.error.reason === "InvalidAsset",
        "Malformed source was not rejected as an invalid asset",
      );
      const partial = await left.inspect();
      check(
        field(left, partial, leftEntity, "Scalar", "value") === 7,
        "Applied prefix or stopped suffix changed",
      );
      check(
        field(left, partial, leftEntity, "MeshInstance", "source") === shared,
        "Rejected source replaced the producer",
      );
      check(
        partial.resources.length === 1 &&
          partial.resources[0]!.id === resource.id,
        "Rejected source changed retained demand",
      );
      const peer = await right.inspect();
      await right.waitForFrame(peer.tick);
      successfulBatch(
        await right.batch(write(right, rightEntity, "Scalar", { value: 8 })),
      );
      check(
        (await ready(right)).id === resource.id,
        "Rejection corrupted another World's demand",
      );
      await record("asset-rejection.survival", {
        partial,
        peer: await right.inspect(),
      });
    }

    const dynamicProperty = (source: string, kind: number): Command => ({
      kind: "setDynamicProperty",
      entity: { kind: "handle", id: leftEntity },
      component: left.components.CustomMaterial!.id,
      name: "selected",
      value: { kind: "asset", value: { kind, source, variant: 0 } },
    });
    successfulBatch(
      await left.batch([
        insertComponent(
          left,
          "CustomMaterial",
          { kind: "handle", id: leftEntity },
          {},
        ),
        dynamicProperty(shared, 1),
        ...write(left, leftEntity, "Scalar", { value: 1 }),
      ]),
    );
    const dynamicOperations = [
      ...write(left, leftEntity, "Scalar", { value: 7 }),
      dynamicProperty(`producer://${leftWorld.id}/2/not-an-id`, 2),
      ...write(left, leftEntity, "Scalar", { value: 99 }),
    ];
    await record("asset-rejection.dynamic-request", {
      operations: dynamicOperations,
    });
    const dynamicRejected = await left.batch(dynamicOperations);
    await record("asset-rejection.dynamic-outcome", dynamicRejected);
    check(!dynamicRejected.ok, "Dynamic reference was accepted");
    check(
      dynamicRejected.error.scope === "operation" &&
        dynamicRejected.error.operation === 1,
      "Dynamic reference was rejected before its Core operation",
    );
    const dynamicState = await left.inspect();
    check(
      field(left, dynamicState, leftEntity, "Scalar", "value") === 7,
      "Dynamic reference rejection lost the applied prefix or ran the suffix",
    );
    const selected = dynamicState.entities
      .find((entity) => entity.id === leftEntity)
      ?.components.find(
        (component) =>
          component.component === left.components.CustomMaterial!.id,
      )?.properties?.selected;
    check(
      selected?.kind === "asset" &&
        selected.value.kind === 1 &&
        selected.value.source === shared,
      "Dynamic rejection replaced the previous property",
    );
    check(
      dynamicState.resources.length === 1 &&
        dynamicState.resources[0]!.id === resource.id,
      "Dynamic rejection corrupted shared demand",
    );
    successfulBatch(
      await right.batch(write(right, rightEntity, "Scalar", { value: 9 })),
    );
    check(
      (await ready(right)).id === resource.id,
      "Dynamic rejection damaged the peer World",
    );
    successfulBatch(
      await left.batch([dynamicProperty(`producer://${leftWorld.id}/2/42`, 2)]),
    );
    const dynamicCorrected = await left.inspect();
    check(
      dynamicCorrected.resources.some((item) => item.source === "asset://2/42"),
      "Valid dynamic correction failed to retain demand",
    );
    successfulBatch(
      await left.batch([
        {
          kind: "removeDynamicProperty",
          entity: { kind: "handle", id: leftEntity },
          component: left.components.CustomMaterial!.id,
          name: "selected",
        },
      ]),
    );
    check(
      (await left.inspect()).resources.length === 1,
      "Dynamic property release lost shared demand or retained stale demand",
    );
    await record("asset-rejection.dynamic-recovered", {
      dynamicState,
      dynamicCorrected,
      peer: await right.inspect(),
    });

    const corrected = successfulBatch(
      await left.batch(
        write(left, leftEntity, "MeshInstance", {
          source: `producer://${leftWorld.id}/1/42`,
        }),
      ),
    );
    const pending = await left.inspect();
    check(
      pending.resources.some((resource) => resource.source === "asset://1/42"),
      "Well-formed pending source was rejected",
    );
    check(
      (await ready(right)).id === resource.id,
      "Correction retired a shared peer resource",
    );
    successfulBatch(
      await left.batch(write(left, leftEntity, "MeshInstance", { source: "" })),
    );
    const cleared = await left.inspect();
    check(
      cleared.resources.length === 0,
      "Empty source failed to release demand",
    );
    successfulBatch(
      await left.batch(
        write(left, leftEntity, "MeshInstance", { source: shared }),
      ),
    );
    check(
      (await ready(left)).id === resource.id,
      "Valid recovery changed the shared resource identity",
    );
    await left.close();
    await host.destroyWorld(leftWorld.reference);
    check(
      (await ready(right)).id === resource.id,
      "World cleanup released the surviving peer's resource",
    );
    successfulBatch(
      await right.batch(
        write(right, rightEntity, "MeshInstance", { source: "" }),
      ),
    );
    const released = await right.inspect();
    check(released.resources.length === 0, "Final consumer left demand behind");
    const worlds = await host.listWorlds();
    check(
      worlds.some((world) => world.id === rightWorld.id),
      "Host stopped serving other Worlds",
    );
    await record("asset-rejection.recovered", {
      corrected,
      pending,
      cleared,
      released,
      worlds,
    });
    return {
      rejected: 3,
      dynamicRejected: true,
      sharedResource: resource.id.toString(),
      recovered: true,
    };
  } finally {
    await Promise.all([left.close(), right.close()]);
    await host.destroyWorld(rightWorld.reference);
  }
}
