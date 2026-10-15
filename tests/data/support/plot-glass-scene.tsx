import { cybertronTokens } from "@ipp/react/gui-cybertron";
/** Native Plot/data fixtures with optional public authored material siblings. */
import {
  canvasOutput,
  type RootBinding,
  type HostClientBase,
  type Client,
} from "@ipp/client";
import {
  createRoot,
  Entity,
  Asset,
  assetRef,
  CustomMaterial,
  MeshInstance,
  UnlitMaterial,
  Transform,
  type ReactWorldRoot,
} from "@ipp/react";
import { Paint } from "../../../packages/ipp-react/src/gui/components.js";
import { glassColor } from "../../../packages/ipp-react/src/gui-glass/palette.js";
import {
  GlassChartAssets,
  GlassChartPaint,
  GlassChartMaterial,
  type GlassChartFinishProps,
} from "../../../packages/ipp-react/src/gui-glass/chart.js";
import {
  openPlot2d,
  closePlot2d,
  readyPlot2d,
  PLOT_2D_EXTENT,
  type Plot2dScene,
  type Plot2dContract,
} from "./plot-2d-scene.js";
import {
  openPlot3d,
  closePlot3d,
  readyPlot3d,
  plot3dView,
  type Plot3dScene,
  type Plot3dContract,
} from "./plot-3d-scene.js";
import {
  componentFields,
  successfulBatch,
} from "../../../examples/demos/scenes/charts/shared/commands.js";
import type { PlotFrame } from "./plot-capture.js";

export interface PlotGlassScene {
  planar: Plot2dScene;
  spatial: Plot3dScene;
  binding: RootBinding;
  roots: ReactWorldRoot[];
  backgroundRoot?: ReactWorldRoot;
}

export async function patchPlot(
  client: Client,
  entity: bigint,
  component: string,
  fields: Record<string, number | boolean>,
) {
  successfulBatch(
    await client.batch(
      componentFields(client, component, fields).map((field) => ({
        kind: "setField",
        entity: { kind: "handle", id: entity },
        component: client.components[component]!.id,
        field,
      })),
    ),
  );
}

/** Keep the same callout while toggling only the native mark highlight. */
export async function highlightPlotGlassBar(
  scene: PlotGlassScene,
  highlighted: boolean,
) {
  const chart = scene.spatial.charts.find((item) => item.name === "grid-bars")!;
  const component = chart.client.components.PlotGridBars3d!;
  successfulBatch(
    await chart.client.batch([
      {
        kind: "setField",
        entity: { kind: "handle", id: chart.entity },
        component: component.id,
        field: {
          offset: component.fields.labels!.offset,
          value: {
            kind: "rows",
            value: scene.spatial.contract.encodeRowsTable(
              component.fields.labels!.rows!,
              {
                nextSlot: 1,
                rows: new Map([
                  [
                    0,
                    {
                      series: 0,
                      row_id: "6",
                      text: "B / 85 UNITS",
                      offset: [1, -2.4],
                      highlighted,
                      connector: true,
                    },
                  ],
                ]),
              },
            ),
          },
        },
      },
    ]),
  );
  return chart.binding;
}

export async function openPlotGlass(
  host: HostClientBase<Client>,
  contract: Plot2dContract & Plot3dContract,
  font: Uint8Array<ArrayBuffer>,
  name: string,
): Promise<PlotGlassScene> {
  // Generated token types retain default numeric literals; this fixture supplies
  // an ordinary authored palette without changing the runtime contract.
  const tokens = {
    ...cybertronTokens,
    accent: glassColor("#00ACDB"),
    neutral: glassColor("#52758D"),
    amber: glassColor("#D8AD63"),
    error: glassColor("#628ED3"),
  };
  const planar = await openPlot2d(host, contract, font, name, tokens);
  let spatial: Plot3dScene | undefined;
  try {
    // This optional specimen styling applies before both baseline and glass.
    // Keep native highlighted rows; the Frame owns their authored accent.
    const highlight = glassColor("#B9E7FA");
    const grid = glassColor("#1A4A62");
    for (const chart of planar.charts)
      await patchPlot(planar.client, chart.entity, "PlotFrame2d", {
        highlight_red: highlight[0],
        highlight_green: highlight[1],
        highlight_blue: highlight[2],
        highlight_alpha: 1,
        grid_red: grid[0],
        grid_green: grid[1],
        grid_blue: grid[2],
        grid_alpha: 0.6,
      });
    spatial = await openPlot3d(host, contract, font, name, tokens);
    for (const chart of spatial.charts)
      await patchPlot(chart.client, chart.camera, "Camera", {
        ortho_height: 17,
      });
    for (const chart of spatial.charts)
      await patchPlot(chart.client, chart.entity, "PlotFrame3d", {
        mark_outlines: false,
      });
    const binding = await host.setRootOutput(
      canvasOutput(planar.world.reference),
      PLOT_2D_EXTENT,
    );
    const roots = [
      createRoot(planar.client),
      ...spatial.charts.map((chart) => createRoot(chart.client)),
    ];
    await readyPlot2d(planar);
    await readyPlot3d(spatial);
    return { planar, spatial, binding, roots };
  } catch (error) {
    if (spatial) await closePlot3d(spatial);
    await closePlot2d(planar);
    throw error;
  }
}

export async function finishPlotGlass(
  scene: PlotGlassScene,
  finish?: GlassChartFinishProps,
  replacement = false,
  dimension?: 2 | 3,
) {
  const prefix = replacement ? "replacement-chart" : "glass-chart";
  if (dimension !== 3)
    await scene.roots[0]!.render(
      finish === undefined ? null : (
        <>
          <GlassChartAssets id={prefix} />
          {scene.planar.charts.map((chart) => (
            <Entity key={chart.source} bindTo={chart.source.split("/").at(-1)!}>
              <GlassChartPaint
                source={{ assetId: `${prefix}-paint` }}
                {...finish}
              />
            </Entity>
          ))}
        </>
      ),
    );
  for (const [index, chart] of scene.spatial.charts.entries())
    if (dimension !== 2)
      await scene.roots[index + 1]!.render(
        finish === undefined ? null : (
          <>
            <GlassChartAssets id={prefix} />
            <Entity bindTo={chart.name}>
              <GlassChartMaterial
                source={{ assetId: `${prefix}-mesh` }}
                {...finish}
              />
            </Entity>
          </>
        ),
      );
  if (finish !== undefined) {
    const deadline = performance.now() + 30_000;
    for (const [index, root] of scene.roots.entries()) {
      if ((dimension === 2 && index > 0) || (dimension === 3 && index === 0))
        continue;
      const id = `${prefix}-${index === 0 ? "paint" : "mesh"}`;
      while (root.getAsset(id)?.status !== "loaded") {
        const status = root.getAsset(id)?.status;
        if (status === "failed" || performance.now() > deadline)
          throw new Error(`Plot glass asset ${id}: ${status}`);
        await new Promise((resolve) => setTimeout(resolve, 10));
      }
    }
  }
}

export async function capturePlotGlassFamilies(
  scene: PlotGlassScene,
  capture: (label: string, binding: RootBinding) => Promise<PlotFrame>,
  prefix: string,
  dimension?: 2 | 3,
) {
  const images: Record<string, PlotFrame> = {};
  const take = async (name: string, binding: RootBinding) => {
    images[name] = await capture(`${prefix}-${name}`, binding);
  };
  if (dimension !== 3) {
    await take("2d-line-bars-pie", scene.binding);
    const pie = scene.planar.charts.find(
      (chart) => chart.component === "PlotPie2d",
    )!;
    await patchPlot(scene.planar.client, pie.entity, pie.component, {
      inner_radius: 0.5,
    });
    await take("2d-line-bars-donut", scene.binding);
    await patchPlot(scene.planar.client, pie.entity, pie.component, {
      inner_radius: 0,
    });
  }
  for (const chart of dimension === 2 ? [] : scene.spatial.charts) {
    await take(`3d-${chart.name}`, chart.binding);
    if (chart.component === "PlotHeightSurface3d") {
      await patchPlot(chart.client, chart.entity, chart.component, {
        wireframe: false,
      });
      await take("3d-height-surface-solid", chart.binding);
      await patchPlot(chart.client, chart.entity, chart.component, {
        wireframe: true,
      });
    }
    if (chart.component === "PlotPoints3d") {
      await patchPlot(chart.client, chart.entity, chart.component, {
        marker_shape: 0,
      });
      await take("3d-point-cubes", chart.binding);
      await patchPlot(chart.client, chart.entity, chart.component, {
        marker_shape: 1,
      });
    }
    if (chart.component === "PlotPie3d") {
      await pieDimensions(scene, false);
      await take("3d-fixed-pie", chart.binding);
      await pieDimensions(scene, true);
    }
  }
  return images;
}

/** Corrupt actual transferred shader bytes, exercising native asset decode failure. */
export async function failPlotGlassPaint(
  scene: PlotGlassScene,
  dimension: 2 | 3 = 2,
) {
  const root = scene.roots[dimension === 2 ? 0 : 1]!;
  const corrupt = new Uint8Array([0, 1, 2]);
  await root.render(
    <>
      <Asset
        id="broken-chart-paint"
        kind={13}
        data={corrupt}
        encode={(value) => value}
      />
      <Entity bindTo={dimension === 2 ? "bars" : "grid-bars"}>
        {dimension === 2 ? (
          <Paint source={assetRef("broken-chart-paint")} />
        ) : (
          <CustomMaterial
            source={assetRef("broken-chart-paint")}
            receives_light={true}
          />
        )}
      </Entity>
    </>,
  );
  const deadline = performance.now() + 30_000;
  while (root.getAsset("broken-chart-paint")?.status !== "failed") {
    if (performance.now() > deadline)
      throw new Error("Corrupt chart paint did not fail decoding");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

async function pieDimensions(scene: PlotGlassScene, variable: boolean) {
  const chart = scene.spatial.charts.find(
    (item) => item.component === "PlotPie3d",
  )!;
  const view = await plot3dView(scene.spatial, chart.name);
  const number = (row: (typeof view.rows)[number], column: string) => {
    const value =
      row.values[view.columns.findIndex((item) => item.name === column)];
    if (!value?.valid || value.value.kind !== "f32")
      throw new Error(`Missing pie ${column}`);
    return value.value.value;
  };
  const sum = view.rows.reduce((total, row) => total + number(row, "value"), 0);
  const series = {
    nextSlot: 1,
    rows: new Map([
      [
        0,
        {
          name: "Group A",
          x: "x",
          y: "y",
          z: "z",
          value: "value",
          radius: variable ? "radius" : "",
          height: variable ? "height" : "",
          color_column: "color",
          color: [0, 0.8, 1, 1],
          visible: true,
        },
      ],
    ]),
  };
  const labels = {
    nextSlot: view.rows.length,
    rows: new Map(
      view.rows.map((row, slot) => [
        slot,
        {
          series: 0,
          row_id: row.id.toString(),
          text: `${String.fromCharCode(65 + slot)} ${Number(((number(row, "value") / sum) * 100).toFixed(1))}% / R${variable ? Number(number(row, "radius").toFixed(1)) : 2.5} H${variable ? Number(number(row, "height").toFixed(1)) : 1.2}`,
          offset: [0, 0],
          highlighted: slot === 0,
          connector: true,
        },
      ]),
    ),
  };
  const component = chart.client.components.PlotPie3d!;
  successfulBatch(
    await chart.client.batch([
      ...componentFields(chart.client, "PlotPie3d", {
        radius: 2.5,
        height: 1.2,
      }).map((field) => ({
        kind: "setField" as const,
        entity: { kind: "handle" as const, id: chart.entity },
        component: component.id,
        field,
      })),
      ...(
        [
          ["series", series],
          ["labels", labels],
        ] as const
      ).map(([name, rows]) => ({
        kind: "setField" as const,
        entity: { kind: "handle" as const, id: chart.entity },
        component: component.id,
        field: {
          offset: component.fields[name]!.offset,
          value: {
            kind: "rows" as const,
            value: scene.spatial.contract.encodeRowsTable<object>(
              component.fields[name]!.rows!,
              rows,
            ),
          },
        },
      })),
    ]),
  );
}

export async function closePlotGlass(scene: PlotGlassScene) {
  if (scene.backgroundRoot) {
    await scene.backgroundRoot.render(null);
    await scene.backgroundRoot.unmount();
  }
  for (const root of scene.roots) {
    await root.render(null);
    await root.unmount();
  }
  await scene.planar.host.clearRootOutput(scene.binding);
  await closePlot3d(scene.spatial);
  await closePlot2d(scene.planar);
}

/** Change only opaque scene decoration behind data marks; never sample captured pixels. */
export async function contrastPlotGlass(
  scene: PlotGlassScene,
  dimension: 2 | 3,
  contrasting: boolean,
) {
  if (dimension === 2) {
    const color = contrasting ? [0.7, 0.15, 0.45] : scene.planar.tokens.surface;
    successfulBatch(
      await scene.planar.client.batch(
        componentFields(scene.planar.client, "CanvasStyle", {
          red: color[0]!,
          green: color[1]!,
          blue: color[2]!,
        }).map((field) => ({
          kind: "setField",
          entity: { kind: "symbol", symbol: "panel-2" },
          component: scene.planar.client.components.CanvasStyle!.id,
          field,
        })),
      ),
    );
  } else {
    const chart = scene.spatial.charts.find(
      (item) => item.name === "grid-bars",
    )!;
    const root = (scene.backgroundRoot ??= createRoot(chart.client));
    const source = "ipp://mesh/cube?width=1&height=1&length=1";
    await root.render(
      contrasting ? (
        <Entity id="contrast-backdrop">
          <Transform x={5} y={2} z={-20} sx={100} sy={100} sz={0.1} />
          <MeshInstance source={source} />
          <UnlitMaterial r={0.7} g={0.15} b={0.45} />
        </Entity>
      ) : null,
    );
    if (contrasting) {
      const deadline = performance.now() + 30_000;
      while (
        !(await chart.client.inspect()).resources.some(
          (resource) =>
            resource.source === source && resource.status === "loaded",
        )
      ) {
        if (performance.now() > deadline)
          throw new Error("Contrast backdrop asset did not load");
        await new Promise((resolve) => setTimeout(resolve, 10));
      }
    }
  }
}
