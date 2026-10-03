/** Real-source selected-view refinement; camera poses are ordinary authored values. */
import type { FieldWrite, PickingWorldClient } from "@ipp/client";
import { defineClient } from "./client.js";
import { image, settled } from "./presentation.js";
import {
  componentFields,
  successfulBatch,
} from "../../examples/chart-showcase/commands.js";
import {
  openPlot3d,
  readyPlot3d,
  rotatePlot3d,
  changePlot3d,
  closePlot3d,
  PLOT_3D_EXTENT,
  type Plot3dContract,
  type Plot3dScene,
} from "../../examples/chart-showcase/plot-3d-scene.js";
import {
  openPlot3dSheet,
  closePlot3dSheet,
  type Plot3dSheet,
} from "../../examples/chart-showcase/sheet-3d.js";

interface Study {
  scene: Plot3dScene;
  sheet?: Plot3dSheet;
  changed: boolean;
}

async function pose(scene: Plot3dScene, mode: string) {
  if (mode === "baseline" || mode === "rotated")
    return rotatePlot3d(scene, mode === "rotated");
  const eye = mode === "below" ? [-10, -8, -12] : [-10, 9, -12];
  const delta = eye.map((value, axis) => value - [5, 2, 5][axis]!);
  const yaw = Math.atan2(delta[0]!, delta[2]!);
  const pitch = -Math.atan2(delta[1]!, Math.hypot(delta[0]!, delta[2]!));
  const cy = Math.cos(yaw / 2),
    sy = Math.sin(yaw / 2),
    cx = Math.cos(pitch / 2),
    sx = Math.sin(pitch / 2);
  for (const chart of scene.charts)
    successfulBatch(
      await chart.client.batch(
        componentFields(chart.client, "Transform", {
          x: eye[0]!,
          y: eye[1]!,
          z: eye[2]!,
          qx: cy * sx,
          qy: sy * cx,
          qz: -sy * sx,
          qw: cy * cx,
        }).map((field: FieldWrite) => ({
          kind: "setField",
          entity: { kind: "handle", id: chart.camera },
          component: chart.client.components.Transform!.id,
          field,
        })),
      ),
    );
}

export default defineClient<Study>({
  async open(context) {
    const scene = await openPlot3d(
      context.host,
      context.contract as unknown as Plot3dContract,
      await context.font(),
      context.name,
    );
    // One ordinary camera margin for the narrower standalone viewport, across
    // every pose. Physical fonts and runtime placement remain unchanged.
    for (const chart of scene.charts)
      successfulBatch(
        await chart.client.batch(
          componentFields(chart.client, "Camera", { ortho_height: 17 }).map(
            (field) => ({
              kind: "setField",
              entity: { kind: "handle", id: chart.camera },
              component: chart.client.components.Camera!.id,
              field,
            }),
          ),
        ),
      );
    return { scene, changed: false };
  },
  async capture(state, context, args) {
    if (args.includes("changed") && !state.changed) {
      await changePlot3d(state.scene);
      state.changed = true;
    }
    if (args.includes("sheet") && !state.sheet)
      state.sheet = await openPlot3dSheet(
        state.scene,
        (context.contract as unknown as Plot3dContract).GUI_SKIN_TOKENS,
        await context.font(),
        context.name,
      );
    const mode =
      args.find((arg) =>
        ["baseline", "rotated", "opposite", "below"].includes(arg),
      ) ?? "baseline";
    await pose(state.scene, mode);
    await readyPlot3d(state.scene);
    const images: Record<string, ReturnType<typeof image>> = {};
    const report: Record<string, unknown> = {};
    if (state.sheet && args.includes("sheet"))
      images["charts-3d"] = await context.capture(state.sheet.binding);
    else
      for (const chart of state.scene.charts) {
        const binding = await context.host.setRootOutput(
          chart.binding.output,
          PLOT_3D_EXTENT,
        );
        try {
          images[chart.name] = await context.present(binding, async (view) => {
            const capture = await settled(context.host, view, [binding.output]);
            const pick =
              chart.name === "grid-bars" &&
              ["baseline", "rotated"].includes(mode)
                ? await (chart.client as unknown as PickingWorldClient).query({
                    type: "GeometryPickQuery",
                    view: { kind: "bound", binding },
                    x:
                      (480 +
                        (((mode === "rotated" ? 445 : 420) - 480) * 14.5) /
                          (state.sheet ? 12.5 : 17)) /
                      960,
                    y:
                      (380 +
                        (((mode === "rotated" ? 360 : 330) - 380) * 14.5) /
                          (state.sheet ? 12.5 : 17)) /
                      760,
                  })
                : undefined;
            report[chart.name] = {
              drawCalls: capture.drawCalls,
              triangles: capture.triangles,
              failedDrawCalls: capture.failedDrawCalls,
              ...(pick?.type === "GeometryPickResultEvent" && pick.ok
                ? {
                    pick: pick.hit
                      ? {
                          component: pick.hit.component,
                          series: pick.hit.row?.series,
                          rowId: pick.hit.row?.rowId.toString(),
                        }
                      : null,
                  }
                : {}),
            };
            return image(capture);
          });
        } finally {
          await context.host.clearRootOutput(binding);
        }
      }
    return {
      images,
      report: { mode, changed: state.changed, views: report },
      summary: [
        "Projected radial pie callouts and camera-selected far Cartesian stations",
        "Retained source rows and geometry; ordinary camera transforms only",
      ],
    };
  },
  async close(state, context) {
    if (state.sheet) await closePlot3dSheet(state.sheet, context.host);
    await closePlot3d(state.scene);
  },
});
