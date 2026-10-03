/** Load with the shared native Host; captures are actual composed runtime output. */
import { canvasOutput } from "@ipp/client";
import { defineClient } from "../../tools/shared-host/client.js";
import {
  openPlot2d,
  readyPlot2d,
  changePlot2d,
  closePlot2d,
  PLOT_2D_EXTENT,
  type Plot2dContract,
  type Plot2dScene,
} from "./plot-2d-scene.js";
import {
  openPlot3d,
  readyPlot3d,
  changePlot3d,
  rotatePlot3d,
  closePlot3d,
  type Plot3dContract,
  type Plot3dScene,
} from "./plot-3d-scene.js";
import {
  openPlot3dSheet,
  closePlot3dSheet,
  type Plot3dSheet,
} from "./sheet-3d.js";

interface Study {
  readonly flat: Plot2dScene;
  readonly spatial: Plot3dScene;
  readonly sheet: Plot3dSheet;
  readonly binding: import("@ipp/client").RootBinding;
  changed: boolean;
}
export default defineClient<Study>({
  async open(context) {
    const contract = context.contract as unknown as Plot2dContract &
      Plot3dContract;
    const font = await context.font();
    const flat = await openPlot2d(context.host, contract, font, context.name);
    let spatial: Plot3dScene | undefined;
    try {
      spatial = await openPlot3d(context.host, contract, font, context.name);
      const sheet = await openPlot3dSheet(
        spatial,
        contract.GUI_SKIN_TOKENS,
        font,
        context.name,
      );
      const binding = await context.host.setRootOutput(
        canvasOutput(flat.world.reference),
        PLOT_2D_EXTENT,
      );
      return { flat, spatial, sheet, binding, changed: false };
    } catch (error) {
      if (spatial) await closePlot3d(spatial);
      await closePlot2d(flat);
      throw error;
    }
  },
  async capture(state, context, args) {
    if (args.includes("changed") && !state.changed) {
      await changePlot2d(state.flat);
      await changePlot3d(state.spatial);
      state.changed = true;
    }
    await rotatePlot3d(state.spatial, args.includes("rotated"));
    await readyPlot2d(state.flat);
    await readyPlot3d(state.spatial);
    const images: Record<
      string,
      Awaited<ReturnType<typeof context.capture>>
    > = {
      "charts-2d": await context.capture(state.binding),
      "charts-3d": await context.capture(state.sheet.binding),
    };
    const single = state.spatial.charts.find(
      (chart) => chart.name === "single-row",
    )!;
    images["single-row"] = await context.capture(single.binding);
    return {
      images,
      summary: [
        "All seven Plot families authored through React from real Data Service sources",
        "Grid bars and disconnected points share one source with independent bindings",
        "The smooth series parameter is a paused Host animation controller, pinned by seek",
      ],
      report: {
        changed: state.changed,
        sharedSource: state.spatial.charts
          .filter((chart) => ["grid-bars", "point-plot"].includes(chart.name))
          .map((chart) => chart.source),
        parameterController: state.flat.parameterController,
        bindings: await Promise.all(
          state.flat.charts.map((chart) =>
            context.host.datasets.bindingView(
              state.flat.client.session,
              chart.entity,
            ),
          ),
        ),
      },
    };
  },
  async close(state, context) {
    await closePlot3dSheet(state.sheet, context.host);
    await closePlot3d(state.spatial);
    await closePlot2d(state.flat);
  },
});
