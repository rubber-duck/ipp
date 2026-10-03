/** Native GLES visual iteration; shares the real-source fixture with integration. */
import { canvasOutput, type RootBinding } from "@ipp/client";
import {
  PLOT_2D_EXTENT,
  openPlot2d,
  readyPlot2d,
  changePlot2d,
  closePlot2d,
  pickPlot2d,
  type Plot2dContract,
  type Plot2dScene,
} from "../../examples/world-gallery/worlds/charts2d/content.js";
import { defineClient } from "./client.js";

interface State {
  readonly scene: Plot2dScene;
  readonly binding: RootBinding;
  changed: boolean;
}
export default defineClient<State>({
  async open(context) {
    const scene = await openPlot2d(
      context.host,
      context.contract as unknown as Plot2dContract,
      await context.font(),
      context.name,
    );
    const binding = await context.host.setRootOutput(
      canvasOutput(scene.world.reference),
      PLOT_2D_EXTENT,
    );
    return { scene, binding, changed: false };
  },
  async capture(state, context, args) {
    if (args.includes("changed") && !state.changed) {
      await changePlot2d(state.scene);
      state.changed = true;
    }
    await readyPlot2d(state.scene);
    const frame = await context.capture(state.binding);
    const picks = await pickPlot2d(state.scene, state.binding, state.changed);
    return {
      report: {
        picks,
        bindings: await Promise.all(
          state.scene.charts.map(async (chart) => ({
            component: chart.component,
            page: await context.host.datasets.bindingView(
              state.scene.client.session,
              chart.entity,
            ),
          })),
        ),
      },
      images: {
        [state.changed ? "changed" : "baseline"]: frame,
      },
      summary: [
        "Real prepared columns, retained native Canvas paths, and source-row highlights",
      ],
    };
  },
  async close(state) {
    await closePlot2d(state.scene);
  },
});
