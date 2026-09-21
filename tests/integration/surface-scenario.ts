/** Surface lifecycle assertions shared by real native and browser transports. */
import {
  surfaceProperty,
  type AnimationWorldClient,
  type SurfaceWorldClient,
  type DynamicValue,
} from "@ipp/client";
import type { TerminalAssets } from "../../examples/surface-terminal/scene.js";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "./camera-fixtures.js";

export type SurfaceTestClient = SurfaceWorldClient & AnimationWorldClient;

function expect(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

export async function surfaceSnapshot(
  client: SurfaceWorldClient,
  entity: bigint,
) {
  const snapshot = (await client.inspect()).entities.find(
    (item) => item.id === entity,
  );
  expect(snapshot, "Surface entity disappeared");
  const component = snapshot.effective.find(
    (item) => item.component === client.components.Surface!.id,
  );
  expect(component, "Surface component disappeared");
  const bytes = component.fields.items;
  expect(bytes instanceof Uint8Array, "Surface inspection omitted typed items");
  return {
    collection: client.decodeSurfaceItems(bytes),
    properties: component.properties ?? {},
  };
}

export async function waitSurfaceAssets(
  client: SurfaceTestClient,
  sources: readonly string[],
) {
  for (;;) {
    const state = await client.inspect();
    const resources = state.resources.filter((item) =>
      sources.includes(item.source),
    );
    const failed = resources.find((item) => item.status === "failed");
    if (failed)
      throw new Error(
        `Surface asset failed: ${JSON.stringify(failed, (_, value) => (typeof value === "bigint" ? String(value) : value))}`,
      );
    if (
      resources.length >= sources.length &&
      resources.every((item) => item.status === "loaded")
    )
      return;
    await client.waitForFrame(state.tick);
  }
}

export async function exerciseSurfaceLifecycle(
  client: SurfaceTestClient,
  assets: TerminalAssets,
  glyphId: number,
) {
  const ref = { kind: "alias", alias: 1 } as const;
  const entity = aliasId(
    await client.batch([
      createEntity(1, "surface-api"),
      insertComponent(client, "Transform", ref),
      insertComponent(client, "Surface", ref, { width: 4, height: 3 }),
    ]),
    1,
  );
  await client.editSurface({
    action: "insert",
    entity,
    id: 1,
    index: 0,
    content: { kind: "label", text: "A\nAV" },
    style: { asset: assets.font },
  });
  await client.editSurface({
    action: "insert",
    entity,
    id: 2,
    index: 1,
    content: { kind: "drawing" },
    style: { asset: assets.icon },
  });
  await client.editSurface({
    action: "insert",
    entity,
    id: 3,
    index: 2,
    content: { kind: "bitmap", size: [1, 1] },
    style: { asset: assets.bitmap },
  });
  await client.editSurface({
    action: "insert",
    entity,
    id: 4,
    index: 3,
    content: { kind: "glyphRun", glyphs: [{ glyphId, position: [0.4, 0.2] }] },
    style: { asset: assets.font },
  });
  await waitSurfaceAssets(client, [
    assets.font.source,
    assets.icon.source,
    assets.bitmap.source,
  ]);
  await client.editSurface({ action: "move", entity, id: 1, index: 3 });
  expect(
    (await surfaceSnapshot(client, entity)).collection.items
      .map((item) => item.id)
      .join() === "2,3,4,1",
    "Reordering changed identity or content",
  );
  const name = surfaceProperty(1, "opacity");
  const component = client.components.Surface!.id;
  const overlays = successfulBatch(
    await client.batch([
      { kind: "createStateOverlayOwner", alias: 1 },
      {
        kind: "attachEntityOverlayBinding",
        owner: { kind: "alias", alias: 1 },
        alias: 2,
        symbolicId: "surface-api",
        mode: "bound",
      },
      {
        kind: "attachComponentStateOverlay",
        owner: { kind: "alias", alias: 1 },
        binding: { kind: "alias", alias: 2 },
        alias: 3,
        component,
        mode: "bound",
        fields: [],
      },
      {
        kind: "updateDynamicComponentStateOverlay",
        owner: { kind: "alias", alias: 1 },
        overlay: { kind: "alias", alias: 3 },
        properties: { [name]: { kind: "f32", value: 0.25 } },
        clear: [],
      },
    ]),
  );
  await client.editSurface({
    action: "update",
    entity,
    id: 1,
    patch: { opacity: 0.8 },
  });
  const value = async (property: string) =>
    (await surfaceSnapshot(client, entity)).properties[
      property
    ] as DynamicValue;
  expect((await value(name)).value === 0.25, "Producer edit bypassed overlay");
  successfulBatch(
    await client.batch([
      {
        kind: "releaseStateOverlayOwner",
        owner: { kind: "handle", id: overlays.stateOverlays[0]!.id },
      },
    ]),
  );
  expect(
    Math.abs(Number((await value(name)).value) - 0.8) < 1e-6,
    "Overlay withdrawal lost underlying edit",
  );
  const clip = await client.createAsset(
    10,
    client.encodeAnimationClip({
      duration: 1,
      tracks: [
        {
          property: { component, name },
          keys: [
            {
              time: 0,
              value: { kind: "dynamic", value: { kind: "f32", value: 0.2 } },
              interpolation: { kind: "linear" },
            },
            {
              time: 1,
              value: { kind: "dynamic", value: { kind: "f32", value: 0.8 } },
              interpolation: { kind: "step" },
            },
          ],
        },
      ],
    }).buffer,
  );
  await waitSurfaceAssets(client, [clip.source]);
  const controller = await client.createAnimationController({
    speed: 0,
    drivers: [
      {
        source: clip.source,
        track: 0,
        target: entity,
        property: { component, name },
      },
    ],
  });
  await client.controlAnimationController(controller, { action: "play" });
  await client.controlAnimationController(controller, {
    action: "seek",
    time: 0.5,
  });
  expect(
    Math.abs(Number((await value(name)).value) - 0.5) < 1e-5,
    "Item animation did not use the named target",
  );
  await client.editSurface({ action: "move", entity, id: 1, index: 0 });
  expect(
    Math.abs(Number((await value(name)).value) - 0.5) < 1e-5,
    "Reordering invalidated live animation",
  );
  await client.editSurface({ action: "remove", entity, id: 1 });
  await client.editSurface({
    action: "insert",
    entity,
    id: 5,
    index: 0,
    content: { kind: "label", text: "replacement" },
    style: { asset: assets.font },
  });
  const after = await surfaceSnapshot(client, entity);
  expect(
    after.collection.nextId === 6 && !after.properties[name],
    "Removed item identity/property was reused",
  );
  expect(
    after.properties[surfaceProperty(5, "opacity")]?.value === 1,
    "Stale animation retargeted replacement item",
  );
  expect(client.world, "Surface client omitted its World descriptor");
  const foreignWorld = client.world.id === 1n ? 2n : 1n;
  const foreignSource = `producer://${foreignWorld}/17/1`;
  for (const [label, source, message] of [
    ["foreign producer", foreignSource, /another World/],
    ["reserved numeric", "asset://1", /Numeric asset references/],
  ] as const) {
    for (const edit of [
      {
        action: "insert" as const,
        entity,
        id: 6,
        index: 1,
        content: { kind: "label" as const, text: label },
        style: { asset: { ...assets.font, source } },
      },
      {
        action: "update" as const,
        entity,
        id: 5,
        patch: { asset: { ...assets.font, source } },
      },
    ]) {
      let rejection: unknown;
      try {
        await client.editSurface(edit);
      } catch (error) {
        rejection = error;
      }
      expect(
        rejection instanceof Error && message.test(rejection.message),
        `${label} Surface asset reference returned an unexpected result: ${String(rejection)}`,
      );
      expect(
        JSON.stringify(await surfaceSnapshot(client, entity)) ===
          JSON.stringify(after),
        `${label} Surface asset rejection changed component state`,
      );
    }
  }
  let rejected = false;
  try {
    await client.editSurface({ action: "remove", entity, id: 1 });
  } catch {
    rejected = true;
  }
  expect(rejected, "Invalid item edit did not return a correlated error");
  return {
    entity,
    nextId: after.collection.nextId,
    itemIds: after.collection.items.map((item) => item.id),
  };
}
