/** Actual native Plot contours at representative widths and presentation scales. */
import { canvasOutput } from "@ipp/client";
import {
  createRoot,
  Entity,
  ShaderAsset,
  PaintShader,
  assetRef,
} from "@ipp/react";
import { Paint } from "@ipp/react/gui";
import { defineClient } from "../../../../tools/shared-host/client.js";
import {
  openPlot2d,
  readyPlot2d,
  closePlot2d,
  PLOT_2D_EXTENT,
  type Plot2dContract,
} from "../../../data/support/plot-2d-scene.js";
import {
  componentFields,
  successfulBatch,
} from "../../../../examples/demos/scenes/charts/shared/commands.js";
import {
  GLASS_CHART_PAINT,
  GLASS_CHART_PAINT_PARAMETERS,
  GLASS_CHART_LIGHT,
  GLASS_CHART_FINISH,
} from "../../../../packages/ipp-react/src/gui-glass/chart-paint.js";

export default defineClient({
  async open(context) {
    const scene = await openPlot2d(
      context.host,
      context.contract as unknown as Plot2dContract,
      await context.font(),
      context.name,
    );
    const root = createRoot(scene.client);
    const binding = await context.host.setRootOutput(
      canvasOutput(scene.world.reference),
      PLOT_2D_EXTENT,
    );
    await readyPlot2d(scene);
    return { scene, root, binding };
  },
  async capture(state, context) {
    const { scene, root, binding } = state;
    await root.render(
      <>
        <ShaderAsset
          id="contour-paint"
          recipe={{ presentation: true, lighting: true }}
          parameters={GLASS_CHART_PAINT_PARAMETERS}
        >
          <PaintShader>{GLASS_CHART_PAINT}</PaintShader>
        </ShaderAsset>
        {scene.charts.map((chart) => (
          <Entity key={chart.source} bindTo={chart.source.split("/").at(-1)!}>
            <Paint
              source={assetRef("contour-paint")}
              accent={[0, 0, 0, 0]}
              light={GLASS_CHART_LIGHT}
              finish={GLASS_CHART_FINISH}
              tint={[0.18, 0.34, 0.39, 1]}
              lighting={[0, 0, 0, 0]}
            />
          </Entity>
        ))}
      </>,
    );
    const deadline = Date.now() + 30000;
    while (root.getAsset("contour-paint")?.status !== "loaded") {
      if (
        root.getAsset("contour-paint")?.status === "failed" ||
        Date.now() > deadline
      )
        throw new Error("contour paint unavailable");
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
    const patch = async (
      entity: bigint,
      component: string,
      values: Record<string, number>,
    ) =>
      successfulBatch(
        await scene.client.batch(
          componentFields(scene.client, component, values).map((field) => ({
            kind: "setField",
            entity: { kind: "handle", id: entity },
            component: scene.client.components[component]!.id,
            field,
          })),
        ),
      );
    const images: Record<
      string,
      Awaited<ReturnType<typeof context.capture>>
    > = {};
    for (const kind of ["smooth-wide", "pie", "donut"] as const) {
      for (const chart of scene.charts)
        await patch(chart.entity, "CanvasStyle", {
          opacity: chart.source.endsWith(
            kind === "smooth-wide" ? "/smooth" : "/pie",
          )
            ? 1
            : 0,
        });
      const chart = scene.charts.find((chart) =>
        chart.source.endsWith(kind === "smooth-wide" ? "/smooth" : "/pie"),
      )!;
      if (kind === "smooth-wide")
        await patch(chart.entity, chart.component, {
          line_width: 14,
          marker_size: 0,
        });
      else
        await patch(chart.entity, chart.component, {
          inner_radius: kind === "donut" ? 0.45 : 0,
        });
      for (const scale of [0.5, 1, 2]) {
        await patch(chart.entity, "CanvasStyle", {
          x: 54,
          y: 178,
          scale_x: scale,
          scale_y: scale,
        });
        images[`${kind}-scale-${scale}`] = await context.capture(binding);
      }
    }
    return {
      images,
      report: {
        nativePlot: true,
        lineWidth: 14,
        scales: [0.5, 1, 2],
        missingDataGap: true,
        contourApproximation: "existing bands; bounded quadratic refinement",
      },
    };
  },
  async close(state) {
    await state.root.unmount();
    await closePlot2d(state.scene);
  },
});
