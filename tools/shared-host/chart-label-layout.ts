/** Real-source label-layout captures, including intentionally clustered callouts. */
import type { FieldWrite, RowPropertyValue } from "@ipp/client";
import { defineClient } from "./client.js";
import {
  componentFields,
  successfulBatch,
} from "../../examples/world-gallery/worlds/charts/shared/commands.js";
import chartLegibility from "./chart-legibility.js";
import {
  openPlot3d,
  closePlot3d,
  readyPlot3d,
  type Plot3dContract,
  type Plot3dScene,
} from "../../examples/world-gallery/worlds/charts3d/content.js";

type Row = Readonly<Record<string, RowPropertyValue>>;

export default defineClient<Plot3dScene>({
  async open(context, args) {
    const scene = await openPlot3d(
      context.host,
      context.contract as unknown as Plot3dContract,
      await context.font(),
      context.name,
    );
    try {
      if (args.includes("clustered")) {
        const chart = scene.charts.find(
          (chart) => chart.name === "variable-pie",
        )!;
        // Deliberately use nearby authored offsets, never client-built geometry.
        // Runtime layout must separate the resulting projected rectangles.
        const values: Row[] = ["A 40%", "B 30%", "C 20%", "D 10%"].map(
          (text, index) => ({
            series: 0,
            row_id: BigInt(index + 1).toString(),
            text: `${text} / CLUSTER`,
            offset: [0.2, -1.0],
            highlighted: false,
            connector: true,
          }),
        );
        const descriptor = chart.client.components[chart.component]!;
        const labels = descriptor.fields.labels!;
        const field: FieldWrite = {
          offset: labels.offset,
          value: {
            kind: "rows",
            value: scene.contract.encodeRowsTable(labels.rows!, {
              nextSlot: values.length,
              rows: new Map(values.map((value, index) => [index, value])),
            }),
          },
        };
        successfulBatch(
          await chart.client.batch([
            {
              kind: "setField",
              entity: { kind: "handle", id: chart.entity },
              component: descriptor.id,
              field,
            },
          ]),
        );
        await readyPlot3d(scene);
      }
      return scene;
    } catch (error) {
      await closePlot3d(scene);
      throw error;
    }
  },
  async capture(scene, context, args) {
    const result = await chartLegibility.capture(scene, context, args);
    if (!args.includes("micro")) return result;
    const images = { ...result.images };
    // One small authored camera motion after the rotated view, using the real
    // camera transform and retained chart resources. This is a capture fixture,
    // not a client layout loop or a runtime simulation step.
    const eye = [-9.98, 10, 14],
      target = [5, 2, 5];
    const dx = eye[0]! - target[0]!,
      dy = eye[1]! - target[1]!,
      dz = eye[2]! - target[2]!;
    const yaw = Math.atan2(dx, dz),
      pitch = -Math.atan2(dy, Math.hypot(dx, dz));
    const sy = Math.sin(yaw / 2),
      cy = Math.cos(yaw / 2),
      sx = Math.sin(pitch / 2),
      cx = Math.cos(pitch / 2);
    const pose = {
      x: eye[0]!,
      y: eye[1]!,
      z: eye[2]!,
      qx: cy * sx,
      qy: sy * cx,
      qz: -sy * sx,
      qw: cy * cx,
    };
    for (const chart of scene.charts) {
      successfulBatch(
        await chart.client.batch(
          componentFields(chart.client, "Transform", pose).map((field) => ({
            kind: "setField" as const,
            entity: { kind: "handle" as const, id: chart.camera },
            component: chart.client.components.Transform!.id,
            field,
          })),
        ),
      );
      images[`${chart.name}-micro`] = await context.capture(chart.binding);
    }
    return {
      ...result,
      images,
      report: { views: result.report, smallCameraMotion: pose },
    };
  },
  async close(scene) {
    await closePlot3d(scene);
  },
});
