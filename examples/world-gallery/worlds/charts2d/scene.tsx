import { createChartScale } from "../charts/shared/scale.js";
import { canvasOutput } from "@ipp/client";
import {
  GALLERY_SYSTEMS,
  type GallerySceneDefinition,
} from "../../shared/scene.js";
import { mountCharts } from "../charts/shared/mount.js";
import {
  openPlot2d,
  readyPlot2d,
  changePlot2d,
  closePlot2d,
  pinPlot2dParameter,
  type Plot2dContract,
} from "./content.js";

const FONT = "/target/font-assets/shure-tech-mono.ippf";

export const chart2dScene: GallerySceneDefinition = {
  id: "charts2d",
  label: "Charts 2D",
  shortLabel: "Charts 2D",
  description:
    "Explore line, bar and pie plots backed by shared datasets and Host animation.",
  defaultOptions: { changed: false, parameter: 0 },
  actions: ["changeSamples", "setParameter", "resetCamera"],
  world: () => ({ create: { selectedSystems: GALLERY_SYSTEMS } }),
  async mount(context, input) {
    const options = { ...chart2dScene.defaultOptions, ...input };
    const contract = context.contract as unknown as Plot2dContract;
    const font = await context.assets.readBytes(FONT, context.signal);
    const scene = await openPlot2d(
      context.canvas.host,
      contract,
      font,
      `gallery/${context.canvas.client.session}`,
    );
    try {
      context.signal.throwIfAborted();
      if (options.changed) await changePlot2d(scene);
      await pinPlot2dParameter(scene, Number(options.parameter));
      const resize = await createChartScale(scene.client, [1400, 1000]);
      const output = canvasOutput(scene.world.reference);
      const ready = readyPlot2d(scene);
      return mountCharts({
        output,
        options,
        ready,
        resize,
        async update(current, patch) {
          const next = { ...current, ...patch };
          const samplesChanged =
            patch.changed !== undefined && next.changed !== current.changed;
          if (samplesChanged && patch.parameter === undefined)
            next.parameter = patch.changed ? 1 : 0;
          if (samplesChanged) await changePlot2d(scene, Boolean(next.changed));
          // Changing samples also seeks the content's parameter controller.
          // Restore the requested parameter even when its option is unchanged.
          if (samplesChanged || next.parameter !== current.parameter)
            await pinPlot2dParameter(scene, Number(next.parameter));
          await readyPlot2d(scene);
          return next;
        },
        actions: {
          async changeSamples(current) {
            if (!current.changed) await changePlot2d(scene);
            await pinPlot2dParameter(scene, 1);
            await readyPlot2d(scene);
            return { ...current, changed: true, parameter: 1 };
          },
          async setParameter(current, args) {
            const parameter = Number(args);
            if (!Number.isFinite(parameter) || parameter < 0 || parameter > 1)
              throw new Error("Chart parameter must be between zero and one");
            await pinPlot2dParameter(scene, parameter);
            await readyPlot2d(scene);
            return { ...current, parameter };
          },
          async resetCamera(current) {
            return current;
          },
        },
        async inspect() {
          return {
            world: await scene.client.inspect(),
            bindings: await Promise.all(
              scene.charts.map((chart) =>
                scene.host.datasets.bindingView(
                  scene.client.session,
                  chart.entity,
                ),
              ),
            ),
          };
        },
        dispose: () => closePlot2d(scene),
      });
    } catch (error) {
      await closePlot2d(scene);
      throw error;
    }
  },
};

export default chart2dScene;
