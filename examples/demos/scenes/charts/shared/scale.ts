import type { Client, PresentationViewport } from "@ipp/client";
import {
  aliasId,
  componentFields,
  createEntity,
  insertComponent,
  successfulBatch,
} from "./commands.js";

/** Fit fixed authored chart coordinates by an ordinary CanvasStyle parent. */
export async function createChartScale(
  client: Client,
  extent: readonly [number, number],
): Promise<(viewport: PresentationViewport) => Promise<void>> {
  const entities = (await client.inspect()).entities
    .filter((entity) => entity.link.parent === null)
    .sort((left, right) =>
      left.link.order < right.link.order
        ? -1
        : left.link.order > right.link.order
          ? 1
          : 0,
    );
  const parent = { kind: "alias", alias: 1 } as const;
  const outcome = await client.batch([
    createEntity(1, "gallery-chart-layout"),
    insertComponent(client, "CanvasStyle", parent),
    ...entities.map((entity) => ({
      kind: "placeEntity" as const,
      entity: { kind: "handle" as const, id: entity.id },
      placement: { parent, before: null },
    })),
  ]);
  const id = aliasId(outcome, 1);
  return async (viewport) => {
    const width = viewport.width / viewport.devicePixelRatio;
    const height = viewport.height / viewport.devicePixelRatio;
    const scale = Math.min(width / extent[0], height / extent[1]);
    successfulBatch(
      await client.batch(
        componentFields(client, "CanvasStyle", {
          x: (width - extent[0] * scale) / 2,
          y: (height - extent[1] * scale) / 2,
          scale_x: scale,
          scale_y: scale,
        }).map((field) => ({
          kind: "setField" as const,
          entity: { kind: "handle" as const, id },
          component: client.components.CanvasStyle!.id,
          field,
        })),
      ),
    );
  };
}
