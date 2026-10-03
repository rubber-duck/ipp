import { createChartScale } from "../charts/shared/scale.js";
import {
  GALLERY_SYSTEMS,
  type GallerySceneDefinition,
} from "../../shared/scene.js";
import { mountCharts } from "../charts/shared/mount.js";
import {
  openPlot3d,
  readyPlot3d,
  changePlot3d,
  rotatePlot3d,
  closePlot3d,
  type Plot3dContract,
  type Plot3dScene,
} from "./content.js";
import {
  openPlot3dSheet,
  closePlot3dSheet,
  type Plot3dSheet,
} from "./sheet.js";

const FONT = "/target/font-assets/shure-tech-mono.ippf";

async function closeCharts(scene: Plot3dScene, sheet: Plot3dSheet) {
  const failures: unknown[] = [];
  try {
    await closePlot3dSheet(sheet, scene.host);
  } catch (error) {
    failures.push(error);
  }
  try {
    await closePlot3d(scene);
  } catch (error) {
    failures.push(error);
  }
  if (failures.length)
    throw new AggregateError(failures, "Chart Worlds remain owned");
}

export const chart3dScene: GallerySceneDefinition = {
  id: "charts3d",
  label: "Charts 3D",
  shortLabel: "Charts 3D",
  description:
    "Inspect grid bars, a height surface, disconnected points and a variable pie in a composed chart sheet.",
  defaultOptions: { changed: false, rotated: false },
  actions: ["changeSamples", "rotate", "resetCamera"],
  world: () => ({ create: { selectedSystems: GALLERY_SYSTEMS } }),
  async mount(context, input) {
    const options = { ...chart3dScene.defaultOptions, ...input };
    const contract = context.contract as unknown as Plot3dContract;
    const font = await context.assets.readBytes(FONT, context.signal);
    const scene = await openPlot3d(
      context.canvas.host,
      contract,
      font,
      `gallery/${context.canvas.client.session}`,
    );
    let sheet: Plot3dSheet | undefined;
    try {
      context.signal.throwIfAborted();
      sheet = await openPlot3dSheet(
        scene,
        contract.GUI_SKIN_TOKENS,
        font,
        `gallery/${context.canvas.client.session}`,
      );
      const ownedSheet = sheet;
      if (options.changed) await changePlot3d(scene);
      await rotatePlot3d(scene, Boolean(options.rotated));
      const resize = await createChartScale(ownedSheet.client, [1536, 1024]);
      const ready = readyPlot3d(scene);
      return mountCharts({
        output: sheet.binding.output,
        options,
        ready,
        resize,
        async update(current, patch) {
          const next = { ...current, ...patch };
          if (patch.changed !== undefined && next.changed !== current.changed)
            await changePlot3d(scene, Boolean(next.changed));
          if (patch.rotated !== undefined)
            await rotatePlot3d(scene, Boolean(patch.rotated));
          await readyPlot3d(scene);
          return next;
        },
        actions: {
          async changeSamples(current) {
            if (!current.changed) await changePlot3d(scene);
            await readyPlot3d(scene);
            return { ...current, changed: true };
          },
          async rotate(current, args) {
            const rotated =
              args === undefined ? !current.rotated : Boolean(args);
            await rotatePlot3d(scene, rotated);
            return { ...current, rotated };
          },
          async resetCamera(current) {
            await rotatePlot3d(scene, false);
            return { ...current, rotated: false };
          },
        },
        async inspect() {
          return {
            sheet: await ownedSheet.client.inspect(),
            charts: await Promise.all(
              scene.charts.map(async (chart) => ({
                name: chart.name,
                world: await chart.client.inspect(),
                binding: await scene.host.datasets.bindingView(
                  chart.client.session,
                  chart.entity,
                ),
              })),
            ),
          };
        },
        dispose: () => closeCharts(scene, ownedSheet),
      });
    } catch (error) {
      if (sheet) await closeCharts(scene, sheet);
      else await closePlot3d(scene);
      throw error;
    }
  },
};

export default chart3dScene;
