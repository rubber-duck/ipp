import { cybertronTokens } from "@ipp/react/gui-cybertron";
/** Actual native 2D marks for fast material/detail and light comparisons. */
import { canvasOutput } from "@ipp/client";
import { createRoot, Entity } from "@ipp/react";
import {
  GlassChartAssets,
  GlassChartPaint,
} from "../../../../packages/ipp-react/src/gui-glass/chart.js";
import { glassColor } from "../../../../packages/ipp-react/src/gui-glass/palette.js";
import { defineClient } from "../../../../tools/shared-host/client.js";
import { crop, enlarge } from "../../../../tools/shared-host/images.js";
import {
  openPlot2d,
  readyPlot2d,
  closePlot2d,
  PLOT_2D_EXTENT,
  type Plot2dContract,
} from "../../../data/support/plot-2d-scene.js";
import { patchPlot } from "../../../data/support/plot-glass-scene.js";

export default defineClient({
  async open(context) {
    const contract = context.contract as unknown as Plot2dContract;
    const scene = await openPlot2d(
      context.host,
      contract,
      await context.font(),
      context.name,
      {
        ...cybertronTokens,
        accent: glassColor("#00ACDB"),
        neutral: glassColor("#52758D"),
        amber: glassColor("#D8AD63"),
        error: glassColor("#628ED3"),
      },
    );
    const root = createRoot(scene.client);
    const binding = await context.host.setRootOutput(
      canvasOutput(scene.world.reference),
      PLOT_2D_EXTENT,
    );
    const highlight = glassColor("#B9E7FA"),
      grid = glassColor("#1A4A62");
    for (const chart of scene.charts)
      await patchPlot(scene.client, chart.entity, "PlotFrame2d", {
        highlight_red: highlight[0],
        highlight_green: highlight[1],
        highlight_blue: highlight[2],
        highlight_alpha: 1,
        grid_red: grid[0],
        grid_green: grid[1],
        grid_blue: grid[2],
        grid_alpha: 0.6,
      });
    await readyPlot2d(scene);
    return { scene, root, binding };
  },
  async capture(state, context, args) {
    const right = args.includes("right");
    await state.root.render(
      <>
        <GlassChartAssets />
        {state.scene.charts.map((chart) => (
          <Entity key={chart.source} bindTo={chart.source.split("/").at(-1)!}>
            <GlassChartPaint
              light={right ? [1.8, 1.2, 0.75, 1] : [-0.3, -0.2, 0.75, 1]}
            />
          </Entity>
        ))}
      </>,
    );
    const deadline = Date.now() + 30000;
    while (state.root.getAsset("glass-chart-paint")?.status !== "loaded") {
      if (
        state.root.getAsset("glass-chart-paint")?.status === "failed" ||
        Date.now() > deadline
      )
        throw new Error("Chart paint unavailable");
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
    const frame = await context.capture(state.binding);
    return {
      images: {
        charts: frame,
        bars: enlarge(crop(frame, [115, 650, 270, 225]), 3),
        pie: enlarge(crop(frame, [920, 615, 258, 258]), 2),
      },
      report: { nativePlot: true, right },
    };
  },
  async close(state) {
    await state.root.unmount();
    await closePlot2d(state.scene);
  },
});
