/** Real generated Plot geometry proves the blended index order on both devices. */
import {
  Entity,
  ShaderAsset,
  VertexShader,
  FragmentShader,
  CustomMaterial,
  assetRef,
} from "@ipp/react";
import { check } from "../../harness/page/checks.js";
import { patchPlot, type PlotGlassScene } from "../support/plot-glass-scene.js";
import { plot3dView, readyPlot3d } from "../support/plot-3d-scene.js";
import { successfulBatch } from "../../../examples/demos/scenes/charts/shared/commands.js";
import type { PlotCapture } from "../support/plot-capture.js";

export async function assertPlotTriangleOrder(
  scene: PlotGlassScene,
  capture: PlotCapture,
) {
  const spatial = scene.spatial;
  const chart = spatial.charts.find((item) => item.name === "grid-bars")!;
  const rows = (await plot3dView(spatial, chart.name)).rows;
  const near = rows[0]!.id,
    far = rows[1]!.id;
  const edits = rows
    .slice(2)
    .map((row) => ({ operation: "remove" as const, row: row.id }));
  const changes = await spatial.host.datasets.update(spatial.bars, [
    ...edits,
    ...[
      { row: near, z: 7, color: [1, 0, 0, 1] },
      { row: far, z: 3, color: [0, 0, 1, 1] },
    ].map(({ row, z, color }) => ({
      operation: "edit" as const,
      row,
      values: [
        ...[5, 5, z, 5, 1, 1, 1].map((value) => ({
          kind: "f32" as const,
          value,
        })),
        { kind: "vec4" as const, value: color },
      ],
    })),
  ]);
  check(!changes.failure, "Ordering fixture source edit failed");
  const component = spatial.contract.components.PlotGridBars3d!;
  const series = spatial.contract.encodeRowsTable(
    component.fields.series!.rows!,
    {
      nextSlot: 1,
      rows: new Map([
        [
          0,
          {
            name: "ordered glass",
            value: "y",
            radius: "",
            height: "",
            x: "x",
            y: "y",
            z: "z",
            color_column: "color",
            color: [1, 1, 1, 1],
            visible: true,
          },
        ],
      ]),
    },
  );
  const labels = spatial.contract.encodeRowsTable(
    component.fields.labels!.rows!,
    { nextSlot: 0, rows: new Map() },
  );
  successfulBatch(
    await chart.client.batch(
      (
        [
          ["series", series],
          ["labels", labels],
        ] as const
      ).map(([name, value]) => ({
        kind: "setField",
        entity: { kind: "handle", id: chart.entity },
        component: component.id,
        field: {
          offset: component.fields[name]!.offset,
          value: { kind: "rows", value },
        },
      })),
    ),
  );
  await patchPlot(chart.client, chart.entity, "PlotFrame3d", {
    width: 10,
    height: 10,
    depth: 10,
    min_x: 0,
    min_y: 0,
    min_z: 0,
    max_x: 10,
    max_y: 10,
    max_z: 10,
    mark_outlines: false,
  });
  await patchPlot(chart.client, chart.entity, "PlotGridBars3d", {
    bar_width: 4,
    bar_depth: 0.8,
  });
  await patchPlot(chart.client, chart.camera, "Camera", { ortho_height: 10 });
  const root = scene.roots[spatial.charts.indexOf(chart) + 1]!;
  await root.render(
    <>
      <ShaderAsset id="ordering" recipe={{}} parameters={{}}>
        <VertexShader>
          {"void materialVertex(){ippDefaultVertex();}"}
        </VertexShader>
        <FragmentShader>
          {"vec4 materialFragment(){return vec4(v_color,.5);}"}
        </FragmentShader>
      </ShaderAsset>
      <Entity bindTo={chart.name}>
        <CustomMaterial
          source={assetRef("ordering")}
          alpha_mode={2}
          receives_light={false}
          conservative_bounds
        />
      </Entity>
    </>,
  );
  const deadline = performance.now() + 30_000;
  while (root.getAsset("ordering")?.status !== "loaded") {
    check(
      performance.now() < deadline &&
        root.getAsset("ordering")?.status !== "failed",
      "Ordering shader not ready",
    );
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  await root.flush();
  await readyPlot3d(spatial);
  const results = [];
  for (const reverse of [false, true]) {
    await patchPlot(chart.client, chart.camera, "Transform", {
      x: 5,
      y: 2.5,
      z: reverse ? -8 : 18,
      qx: 0,
      qy: reverse ? 1 : 0,
      qz: 0,
      qw: reverse ? 0 : 1,
    });
    const image = await capture(
      reverse ? "ordering-back" : "ordering-front",
      chart.binding,
    );
    // Both bars overlap here. The exact centre lies on two axis grid lines,
    // whose independent scene-depth drawing is not part of this blend probe.
    const offset =
      (Math.floor(image.height * 0.56) * image.width +
        Math.floor(image.width * 0.55)) *
      4;
    const red = image.pixels[offset]!,
      blue = image.pixels[offset + 2]!;
    check(
      (reverse ? blue - red : red - blue) > 30,
      `Transparent triangles ignored camera order: ${JSON.stringify({ reverse, red, blue })}`,
    );
    check(
      Math.min(red, blue) > 90,
      "Far glass layer was occluded by a depth write",
    );
    results.push({ reverse, red, blue });
  }
  return results;
}
