/** Actual Data Service / native Plot families with optional public glass declarations. */
import { defineClient } from "../../../../tools/shared-host/client.js";
import {
  sideBySide,
  type RgbaImage,
} from "../../../../tools/shared-host/images.js";
import {
  openPlotGlass,
  finishPlotGlass,
  capturePlotGlassFamilies,
  closePlotGlass,
  type PlotGlassScene,
} from "../../../data/support/plot-glass-scene.js";
import type { Plot2dContract } from "../../../data/support/plot-2d-scene.js";
import type { Plot3dContract } from "../../../data/support/plot-3d-scene.js";

export default defineClient<PlotGlassScene>({
  async open(context) {
    return openPlotGlass(
      context.host,
      context.contract as unknown as Plot2dContract & Plot3dContract,
      await context.font(),
      context.name,
    );
  },
  async capture(state, context, args) {
    const images: Record<string, RgbaImage> = {};
    const capture = async (
      _label: string,
      binding: PlotGlassScene["binding"],
    ) => context.capture(binding);
    await finishPlotGlass(state);
    const baseline = await capturePlotGlassFamilies(state, capture, "default");
    for (const [name, frame] of Object.entries(baseline))
      images[`default-${name}`] = frame;
    if (!args.includes("baseline")) {
      await finishPlotGlass(state, {});
      const glass = await capturePlotGlassFamilies(state, capture, "glass");
      for (const [name, frame] of Object.entries(glass)) {
        images[`glass-${name}`] = frame;
        images[`comparison-${name}`] = sideBySide([baseline[name]!, frame], 16);
      }
      if (args.includes("verify")) {
        await finishPlotGlass(state, { light: [1.8, 1.2, 0.75, 1] }, false, 2);
        await finishPlotGlass(state, { light: [7, 4, -2, 1] }, false, 3);
        const right = await capturePlotGlassFamilies(state, capture, "right");
        for (const [name, frame] of Object.entries(right))
          images[`right-${name}`] = frame;
        await finishPlotGlass(state, { accent: "#BF9CFF" });
        const violet = await capturePlotGlassFamilies(state, capture, "violet");
        for (const [name, frame] of Object.entries(violet))
          images[`violet-${name}`] = frame;
      }
    }
    return {
      images,
      report: {
        nativeGeometry: true,
        dataServiceSources: true,
        families: Object.keys(baseline),
        material: args.includes("baseline")
          ? "default"
          : "optional authored glass",
        sceneSampling: false,
      },
    };
  },
  async close(state) {
    await closePlotGlass(state);
  },
});
