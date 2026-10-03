/** Native GLES fixture; no client mesh generation or simulation stepping. */
import { defineClient } from "./client.js";
import {
  openPlot3d,
  readyPlot3d,
  rotatePlot3d,
  changePlot3d,
  closePlot3d,
  type Plot3dContract,
  type Plot3dScene,
} from "../../examples/world-gallery/worlds/charts3d/content.js";

interface State {
  scene: Plot3dScene;
  changed: boolean;
}
export default defineClient<State>({
  async open(context) {
    return {
      scene: await openPlot3d(
        context.host,
        context.contract as unknown as Plot3dContract,
        await context.font(),
        context.name,
      ),
      changed: false,
    };
  },
  async capture(state, context, args) {
    if (args.includes("changed") && !state.changed) {
      await changePlot3d(state.scene);
      state.changed = true;
    }
    const rotated = args.includes("rotated");
    await rotatePlot3d(state.scene, rotated);
    await readyPlot3d(state.scene);
    const images: Record<
      string,
      Awaited<ReturnType<typeof context.capture>>
    > = {};
    for (const chart of state.scene.charts)
      images[chart.name] = await context.capture(chart.binding);
    return {
      images,
      summary: [
        `Real native meshes/planes, source-keyed labels; camera ${rotated ? "rotated" : "baseline"}; sources ${state.changed ? "changed" : "baseline"}`,
        "Grid bars and point plots consume independent bindings on the same source",
      ],
    };
  },
  async close(state) {
    await closePlot3d(state.scene);
  },
});
