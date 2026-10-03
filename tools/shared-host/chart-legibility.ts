/** Native view refinement over the maintained real-source 3D Plot fixture. */
import { defineClient } from "./client.js";
import type { PickingWorldClient } from "@ipp/client";
import { image, settled } from "./presentation.js";
import { renderDiagnostics } from "../../packages/ipp-client/src/diagnostics.js";
import {
  openPlot3d,
  readyPlot3d,
  rotatePlot3d,
  changePlot3d,
  closePlot3d,
  type Plot3dContract,
  type Plot3dScene,
} from "../../examples/chart-showcase/plot-3d-scene.js";

export default defineClient<Plot3dScene>({
  async open(context) {
    return openPlot3d(
      context.host,
      context.contract as unknown as Plot3dContract,
      await context.font(),
      context.name,
    );
  },
  async capture(scene, context, args) {
    if (args.includes("changed")) await changePlot3d(scene);
    const images: Record<string, ReturnType<typeof image>> = {};
    const observations: Record<string, unknown> = {};
    // Both views use the same live sources, Worlds and retained chart output.
    for (const rotated of [false, true]) {
      await rotatePlot3d(scene, rotated);
      await readyPlot3d(scene);
      for (const chart of scene.charts) {
        const key = `${chart.name}-${rotated ? "rotated" : "baseline"}`;
        images[key] = await context.present(chart.binding, async (view) => {
          const capture = await settled(context.host, view, [
            chart.binding.output,
          ]);
          const diagnostics = await renderDiagnostics(
            context.host,
          )?.statistics();
          const pick =
            chart.name === "grid-bars"
              ? await (chart.client as unknown as PickingWorldClient).query({
                  type: "GeometryPickQuery",
                  view: { kind: "bound", binding: chart.binding },
                  x: (rotated ? 445 : 420) / 960,
                  y: (rotated ? 360 : 330) / 760,
                })
              : undefined;
          observations[key] = {
            tick: capture.sources[0]?.tick.toString(),
            drawCalls: capture.drawCalls,
            triangles: capture.triangles,
            failedDrawCalls: capture.failedDrawCalls,
            diagnosticsAvailable: diagnostics !== undefined,
            uploadedBytes: diagnostics?.frame.uploadedBytes,
            guiRebuilds: diagnostics?.gui.guiRebuilds,
            guiAllocations: diagnostics?.gui.guiAllocations,
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
      }
    }
    return {
      images,
      report: observations,
      summary: [
        "Anchored camera-facing text, fixed scene grids, retained real 3D meshes",
        "Baseline and rotated views share the same source/binding/geometry lifetimes",
      ],
    };
  },
  async close(scene) {
    await closePlot3d(scene);
  },
});
