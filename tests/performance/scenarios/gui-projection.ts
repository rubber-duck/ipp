import type { Client, HostClientBase, WorldReference } from "@ipp/client";
import {
  aliasId,
  createEntity,
  insertComponent,
} from "../../../examples/demos/scenes/charts/shared/commands.js";
import {
  ATTACHMENTS,
  SCENE,
  SURFACE,
  selectSystems,
} from "../../fixtures/system-selections.js";

import type { Projection } from "./gui-cases.js";

export const boardWidthMetres = 4;
export const curvature = 0.45;
interface Extent {
  width: number;
  height: number;
}

/** Parallel camera with a 1:1 pixel scale at the board's central tangent. */
export function projectPixel(
  x: number,
  y: number,
  { width, height }: Extent,
  projection: Projection,
) {
  if (projection === "canvas" || projection === "flat") return [x, y] as const;
  const dx = x - width / 2,
    dy = y - height / 2;
  const r = projection === "sphere" ? Math.hypot(dx, dy) : Math.abs(dx);
  const angle = ((r * boardWidthMetres) / width) * curvature;
  const scale = angle ? Math.sin(angle) / angle : 1;
  return [
    width / 2 + dx * scale,
    height / 2 + dy * (projection === "sphere" ? scale : 1),
  ] as const;
}

export async function projectBoard(
  host: HostClientBase<Client>,
  board: WorldReference,
  projection: Exclude<Projection, "canvas">,
  extent: Extent,
) {
  const parent = await host.createWorld({
    temporary: true,
    selectedSystems: selectSystems(ATTACHMENTS, SCENE, SURFACE),
  });
  try {
    const scene = await host.openWorld(parent.reference);
    const close = async () => {
      try {
        await scene.close();
      } finally {
        await host.destroyWorld(parent.reference);
      }
    };
    const surface = { kind: "alias", alias: 2 } as const;
    const camera = { kind: "alias", alias: 1 } as const;
    const height = (boardWidthMetres * extent.height) / extent.width;
    const attachment = scene.components.WorldAttachment!;
    const geometry = {
      flat: "FlatSurface",
      cylinder: "CylinderSurface",
      sphere: "SphereSurface",
    }[projection];
    try {
      const batch = await scene.batch([
        createEntity(1, "camera"),
        insertComponent(scene, "Camera", camera, {
          projection: 1,
          ortho_height: height,
          near: 0.1,
          far: 20,
        }),
        createEntity(2, "board"),
        insertComponent(scene, "Transform", surface, { z: -4 }),
        insertComponent(scene, geometry, surface, {
          width: boardWidthMetres,
          height,
          layer_spacing: 0,
          ...(projection === "flat" ? {} : { curvature }),
        }),
        {
          kind: "insertComponent",
          entity: surface,
          component: attachment.id,
          fields: [
            {
              offset: attachment.fields.mode!.offset,
              value: { kind: "u32", value: 1 },
            },
            {
              offset: attachment.fields.child!.offset,
              value: { kind: "world", value: board },
            },
          ],
        },
      ]);
      const output = await host.bindOutput(
        parent.reference,
        aliasId(batch, 1),
        "camera",
      );
      return { output, close };
    } catch (error) {
      try {
        await scene.close();
      } catch (cleanup) {
        throw new AggregateError(
          [error, cleanup],
          "Surface setup and session cleanup failed",
        );
      }
      throw error;
    }
  } catch (error) {
    try {
      await host.destroyWorld(parent.reference);
    } catch (cleanup) {
      throw new AggregateError(
        [error, cleanup],
        "Surface setup and World cleanup failed",
      );
    }
    throw error;
  }
}
