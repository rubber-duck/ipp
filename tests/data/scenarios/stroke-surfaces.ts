/** Real retained plot strokes on curved surfaces, viewed from two eye positions. */
import type { Client, HostClientBase, RootBinding } from "@ipp/client";
import {
  aliasId,
  componentFields,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../../../examples/demos/scenes/charts/shared/commands.js";
import { check } from "../../harness/page/checks.js";

type Frame = { width: number; height: number; pixels: Uint8Array };

function cyanMask(frame: Frame) {
  const mask = new Uint8Array(frame.width * frame.height);
  for (let i = 0; i < mask.length; i++) {
    const p = i * 4;
    mask[i] = Number(
      frame.pixels[p]! < 60 &&
        frame.pixels[p + 1]! > 120 &&
        frame.pixels[p + 2]! > 150,
    );
  }
  return mask;
}

function compareTrace(direct: Frame, reference: Frame) {
  check(
    direct.width === reference.width && direct.height === reference.height,
    "Surface reference changed capture extent",
  );
  const a = cyanMask(direct),
    b = cyanMask(reference);
  const match = (left: Uint8Array, right: Uint8Array) => {
    let ink = 0,
      matched = 0;
    for (let y = 2; y < direct.height - 2; y++)
      for (let x = 2; x < direct.width - 2; x++) {
        if (!left[y * direct.width + x]) continue;
        ink++;
        let near = false;
        for (let dy = -2; dy <= 2; dy++)
          for (let dx = -2; dx <= 2; dx++)
            near ||= !!right[(y + dy) * direct.width + x + dx];
        if (near) matched++;
      }
    check(ink > 100, `Curved plot trace missing: ${ink}`);
    check(
      matched / ink > 0.95,
      `Curved stroke coverage diverges from image reference: ${matched}/${ink}`,
    );
    return { ink, matched };
  };
  return { direct: match(a, b), reference: match(b, a) };
}

export async function exerciseStrokeSurfaces(
  host: HostClientBase<Client>,
  child: Client,
  capture: (label: string, binding: RootBinding) => Promise<Frame>,
  record: (label: string, value: unknown) => Promise<void>,
) {
  const world = await host.createWorld({
    temporary: true,
    selectedSystems: [
      "ipp.animation",
      "ipp.hierarchy",
      "ipp.look-at",
      "ipp.final-propagation",
      "ipp.geometry",
      "ipp.render",
      "ipp.camera",
      "ipp.surface",
      "ipp.world-attachment",
      "ipp.asset-dependencies",
      "ipp.lifecycle-publisher",
    ],
  });
  const client = await host.openWorld(world.reference);
  let binding: RootBinding | undefined;
  try {
    const cameraRef = { kind: "alias" as const, alias: 1 },
      panelRef = { kind: "alias" as const, alias: 2 };
    const attached = client.components.WorldAttachment!;
    const result = successfulBatch(
      await client.batch([
        createEntity(1, "stroke-camera"),
        insertComponent(client, "Transform", cameraRef, { z: 8 }),
        insertComponent(client, "Camera", cameraRef, {
          projection: 1,
          ortho_height: 5.5,
          near: 0.1,
          far: 20,
        }),
        createEntity(2, "stroke-panel"),
        insertComponent(client, "Transform", panelRef),
        insertComponent(client, "CylinderSurface", panelRef, {
          width: 640 / 96,
          height: 400 / 96,
          curvature: 0.2,
          layer_spacing: 0,
        }),
        {
          kind: "insertComponent",
          entity: panelRef,
          component: attached.id,
          fields: [
            {
              offset: attached.fields.child!.offset,
              value: { kind: "world", value: child.worldReference! },
            },
            {
              offset: attached.fields.mode!.offset,
              value: { kind: "u32", value: 1 },
            },
            {
              offset: attached.fields.output!.offset,
              value: { kind: "output", value: null },
            },
          ],
        },
      ]),
    );
    const camera = aliasId(result, 1),
      panel = aliasId(result, 2);
    binding = await host.setRootOutput(
      await host.bindOutput(world.reference, camera, "camera"),
      { width: 640, height: 400, devicePixelRatio: 1 },
    );
    let previous: Frame | undefined;
    for (const shape of ["CylinderSurface", "SphereSurface"] as const) {
      if (shape === "SphereSurface")
        successfulBatch(
          await client.batch([
            {
              kind: "removeComponent",
              entity: { kind: "handle", id: panel },
              component: client.components.CylinderSurface!.id,
            },
            insertComponent(
              client,
              shape,
              { kind: "handle", id: panel },
              {
                width: 640 / 96,
                height: 400 / 96,
                curvature: -0.2,
                layer_spacing: 0,
              },
            ),
          ]),
        );
      for (const eye of [-0.032, 0.032]) {
        successfulBatch(
          await client.batch(
            componentFields(client, "Transform", { x: eye }).map((field) => ({
              kind: "setField",
              entity: { kind: "handle", id: camera },
              component: client.components.Transform!.id,
              field,
            })),
          ),
        );
        const direct = await capture(`stroke-${shape}-${eye}-direct`, binding);
        successfulBatch(
          await client.batch([
            insertComponent(
              client,
              "SurfaceCache",
              { kind: "handle", id: panel },
              {
                force_cache: true,
                resolution_scale: 4,
                max_texture_size: 2048,
              },
            ),
          ]),
        );
        const reference = await capture(
          `stroke-${shape}-${eye}-reference`,
          binding,
        );
        await record(`stroke-${shape}-${eye}`, compareTrace(direct, reference));
        successfulBatch(
          await client.batch([
            {
              kind: "removeComponent",
              entity: { kind: "handle", id: panel },
              component: client.components.SurfaceCache!.id,
            },
          ]),
        );
        if (previous)
          check(
            direct.pixels.some((v, i) => v !== previous!.pixels[i]),
            "Eye/Surface change did not change rendered pixels",
          );
        previous = direct;
      }
    }
  } finally {
    if (binding) await host.clearRootOutput(binding);
    await client.close();
    await host.destroyWorld(world.reference);
  }
}
