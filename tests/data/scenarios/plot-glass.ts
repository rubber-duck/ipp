import { assertPlotTriangleOrder } from "./plot-glass-order.js";
/** Authored finishes exercise actual Plot/data geometry through either maintained transport. */
import type { Client, HostClientBase, PickingWorldClient } from "@ipp/client";
import { check } from "../../harness/page/checks.js";
import {
  openPlotGlass,
  finishPlotGlass,
  capturePlotGlassFamilies,
  closePlotGlass,
  failPlotGlassPaint,
  contrastPlotGlass,
  patchPlot,
  highlightPlotGlassBar,
  type PlotGlassScene,
} from "../support/plot-glass-scene.js";
import {
  assertPlotFinishResponse,
  assertPlotSurfaceDetail,
  assertPlotSurfaceEdges,
  assertPlotRegionUnchanged,
  assertPlotSelection,
  plotPixelDifference,
} from "../support/plot-glass-oracle.js";
import {
  pickPlot2d,
  changePlot2d,
  readyPlot2d,
  type Plot2dContract,
} from "../support/plot-2d-scene.js";
import {
  changePlot3d,
  plot3dView,
  type Plot3dContract,
} from "../support/plot-3d-scene.js";
import type { PlotCapture, PlotFrame } from "../support/plot-capture.js";

import { assertWideSmoothContour } from "../support/plot-contour-oracle.js";

type Contract = Plot2dContract & Plot3dContract;
type RecordEvidence = (name: string, value: unknown) => Promise<void>;

async function planarPicks(scene: PlotGlassScene, changed = false) {
  const picks = await pickPlot2d(scene.planar, scene.binding, changed);
  for (const [name, expected] of Object.entries(
    changed ? { line: 5n, bar: 4n, pie: 2n } : { line: 3n, bar: 2n, pie: 1n },
  )) {
    const result = picks[name as keyof typeof picks];
    check(
      result.ok &&
        result.hit?.row?.rowId === expected &&
        result.hit.row.series === 0,
      `${name}: finish changed source picking`,
    );
  }
  return picks;
}

async function spatialPick(scene: PlotGlassScene) {
  const chart = scene.spatial.charts.find((item) => item.name === "grid-bars")!;
  const eye = [10, 8, 14],
    delta = [5, 6, 9];
  const yaw = Math.atan2(delta[0]!, delta[2]!);
  const pitch = -Math.atan2(delta[1]!, Math.hypot(delta[0]!, delta[2]!));
  const relative = [3.75 - eye[0]!, 4.25 - eye[1]!, 5 - eye[2]!];
  const right = Math.cos(yaw) * relative[0]! - Math.sin(yaw) * relative[2]!;
  const up =
    Math.sin(yaw) * Math.sin(pitch) * relative[0]! +
    Math.cos(pitch) * relative[1]! +
    Math.cos(yaw) * Math.sin(pitch) * relative[2]!;
  const result = await (chart.client as unknown as PickingWorldClient).query({
    type: "GeometryPickQuery",
    view: { kind: "bound", binding: chart.binding },
    x: 0.5 + right / ((17 * 960) / 760),
    y: 0.5 - up / 17,
    includeViewPlane: false,
  });
  check(
    result.ok &&
      result.hit?.entity === chart.entity &&
      result.hit.row?.rowId === 6n &&
      result.hit.row.series === 0,
    "3D finish changed source picking",
  );
  return result;
}

function compareFamilies(
  before: Record<string, PlotFrame>,
  glass: Record<string, PlotFrame>,
  right: Record<string, PlotFrame>,
) {
  return Object.fromEntries(
    Object.entries(before).map(([name, frame]) => {
      const minimum = name.includes("point")
        ? 12
        : name.startsWith("2d")
          ? 500
          : 100;
      const region = name.startsWith("2d")
        ? ([100, 210, 1330, 890] as const)
        : ([100, 120, 850, 620] as const);
      return [
        name,
        assertPlotFinishResponse(
          frame,
          glass[name]!,
          right[name]!,
          region,
          minimum,
        ),
      ];
    }),
  );
}

async function curvedMarkScales(scene: PlotGlassScene, capture: PlotCapture) {
  const pie = scene.planar.charts.find(
    (item) => item.component === "PlotPie2d",
  )!;
  const smooth = scene.planar.charts.find((item) =>
    item.source.endsWith("/smooth"),
  )!;
  const client = scene.planar.client as unknown as PickingWorldClient;
  await patchPlot(scene.planar.client, smooth.entity, "PlotLine2d", {
    line_width: 14,
    marker_size: 0,
  });
  try {
    for (const scale of [0.5, 1, 2]) {
      await patchPlot(scene.planar.client, smooth.entity, "CanvasStyle", {
        scale_x: scale,
        scale_y: scale,
      });
      await patchPlot(scene.planar.client, pie.entity, "CanvasStyle", {
        scale_x: scale,
        scale_y: scale,
      });
      await finishPlotGlass(scene, undefined, false, 2);
      const baseline = await capture(`curve-${scale}-default`, scene.binding);
      await finishPlotGlass(scene, {}, false, 2);
      const glass = await capture(`curve-${scale}-glass`, scene.binding);
      assertWideSmoothContour(glass, { scale, baseline });
      // Outside the circle, inside its rectangular bounds. These coordinates
      // come only from the authored frame/margins, not runtime generated paths.
      const x = Math.round(732 + (317 - 0.9 * 124) * scale);
      const y = Math.round(595 + (148 - 0.9 * 124) * scale);
      assertPlotRegionUnchanged(baseline, glass, [x - 2, y - 2, x + 2, y + 2]);
      const pick = await client.query({
        type: "GeometryPickQuery",
        view: { kind: "bound", binding: scene.binding },
        x: x / 1400,
        y: y / 1000,
        includeViewPlane: false,
      });
      check(
        pick.ok && pick.hit?.entity !== pie.entity,
        `Pie rectangular corner became a data hit at scale ${scale}`,
      );
    }
  } finally {
    await patchPlot(scene.planar.client, smooth.entity, "PlotLine2d", {
      line_width: 2.5,
      marker_size: 6,
    });
    await patchPlot(scene.planar.client, smooth.entity, "CanvasStyle", {
      scale_x: 1,
      scale_y: 1,
    });
    await patchPlot(scene.planar.client, pie.entity, "CanvasStyle", {
      scale_x: 1,
      scale_y: 1,
    });
    await finishPlotGlass(scene, {}, false, 2);
  }
}

/** Separate cases keep every backend/family inside its own environment and timeout. */
export async function exercisePlotGlass(
  host: HostClientBase<Client>,
  contract: Contract,
  font: Uint8Array<ArrayBuffer>,
  capture: PlotCapture,
  record: RecordEvidence,
  family: "glass-2d" | "glass-3d" | "glass-lifecycle",
) {
  const scene = await openPlotGlass(host, contract, font, family);
  const dimension =
    family === "glass-lifecycle" ? undefined : family === "glass-3d" ? 3 : 2;
  try {
    const beforeView = await host.datasets.bindingView(
      scene.planar.client.session,
      scene.planar.charts[0]!.entity,
    );
    const spatialView = await plot3dView(scene.spatial, "grid-bars");
    const before = await capturePlotGlassFamilies(
      scene,
      capture,
      "default",
      dimension,
    );
    await finishPlotGlass(scene, {}, false, dimension);
    const glass = await capturePlotGlassFamilies(
      scene,
      capture,
      "glass",
      dimension,
    );
    if (dimension !== 2)
      await record(
        "surface-edges",
        assertPlotSurfaceEdges(
          glass["3d-height-surface-solid"]!,
          glass["3d-height-surface"]!,
        ),
      );
    if (dimension === 2)
      await record(
        "surface-detail",
        assertPlotSurfaceDetail(glass["2d-line-bars-pie"]!),
      );
    const picks =
      dimension === 2 ? await planarPicks(scene) : await spatialPick(scene);
    await record("glass-picks", picks);
    if (dimension === 3) {
      const binding = await highlightPlotGlassBar(scene, false);
      const idle = await capture("selection-idle", binding);
      await highlightPlotGlassBar(scene, true);
      const selected = await capture("selection-active", binding);
      await record("selection-contrast", assertPlotSelection(idle, selected));
      await spatialPick(scene);
    }
    if (family === "glass-lifecycle") {
      await finishPlotGlass(scene, {}, true);
      const replacement = await capture("replacement", scene.binding);
      check(
        plotPixelDifference(glass["2d-line-bars-pie"]!, replacement).mean < 0.1,
        "Equivalent shader replacement changed data marks",
      );
      await planarPicks(scene);
      const grid = scene.spatial.charts.find(
        (item) => item.name === "grid-bars",
      )!;
      const meshReplacement = await capture("mesh-replacement", grid.binding);
      check(
        plotPixelDifference(glass["3d-grid-bars"]!, meshReplacement).mean < 0.1,
        "Equivalent material replacement changed spatial marks",
      );
      await failPlotGlassPaint(scene);
      await failPlotGlassPaint(scene, 3);
      const failed = await capture("failed-shader", scene.binding);
      assertPlotRegionUnchanged(
        before["2d-line-bars-pie"]!,
        failed,
        [36, 20, 1364, 110],
      );
      check(
        plotPixelDifference(before["2d-line-bars-pie"]!, failed).mean < 0.1,
        "Failed paint did not visibly fall back to default data marks",
      );
      await planarPicks(scene);
      await spatialPick(scene);
      const failedMesh = await capture("failed-mesh-shader", grid.binding);
      assertPlotRegionUnchanged(
        before["3d-grid-bars"]!,
        failedMesh,
        [25, 285, 145, 307],
      );
      check(
        plotPixelDifference(before["3d-grid-bars"]!, failedMesh).mean < 0.1,
        "Failed material did not visibly fall back to default spatial data marks",
      );
      await finishPlotGlass(scene, {}, true);
      const recovered = await capture("recovered", scene.binding);
      check(
        plotPixelDifference(replacement, recovered).mean < 0.1,
        "Recovering failed paint changed native geometry",
      );
      const recoveredMesh = await capture("recovered-mesh", grid.binding);
      check(
        plotPixelDifference(meshReplacement, recoveredMesh).mean < 0.1,
        "Recovering failed material changed spatial marks",
      );
      await finishPlotGlass(scene);
      const removed = await capture("removed", scene.binding);
      check(
        plotPixelDifference(before["2d-line-bars-pie"]!, removed).mean < 0.1,
        "Removing paint did not restore default native presentation",
      );
      const removedMesh = await capture("removed-mesh", grid.binding);
      check(
        plotPixelDifference(before["3d-grid-bars"]!, removedMesh).mean < 0.1,
        "Removing material did not restore default spatial presentation",
      );
      await record("glass-lifecycle", {
        replacement: true,
        failedTransferDecode: true,
        recovered: true,
        removed: true,
      });
      return { family, lifecycle: true };
    }
    const opacityName = dimension === 2 ? "2d-line-bars-pie" : "3d-grid-bars";
    const opacityBinding =
      dimension === 2
        ? scene.binding
        : scene.spatial.charts.find((item) => item.name === "grid-bars")!
            .binding;
    await contrastPlotGlass(scene, dimension!, true);
    const contrast = await capture("contrast-backdrop", opacityBinding);
    const interior =
      dimension === 2
        ? ([199, 740, 226, 850] as const)
        : ([394, 345, 412, 420] as const);
    const background =
      dimension === 2
        ? ([117, 635, 132, 650] as const)
        : ([860, 60, 900, 90] as const);
    const opacity = plotPixelDifference(
      glass[opacityName]!,
      contrast,
      interior,
    );
    check(
      dimension === 2 ? opacity.mean < 0.1 : opacity.mean > 5,
      `Glass data transmission is incorrect: ${opacity.mean}`,
    );
    check(
      plotPixelDifference(glass[opacityName]!, contrast, background).mean > 20,
      "Contrast backdrop did not change visible scene pixels",
    );
    await contrastPlotGlass(scene, dimension!, false);
    await record(
      dimension === 2
        ? "backed-planar-interior"
        : "transparent-spatial-interior",
      { opacity, interior, background },
    );
    await finishPlotGlass(
      scene,
      { light: dimension === 2 ? [1.8, 1.2, 0.75, 1] : [7, 4, -2, 1] },
      false,
      dimension,
    );
    const right = await capturePlotGlassFamilies(
      scene,
      capture,
      "right-light",
      dimension,
    );
    const response = compareFamilies(before, glass, right);
    if (dimension === 2)
      assertPlotRegionUnchanged(
        before["2d-line-bars-pie"]!,
        glass["2d-line-bars-pie"]!,
        [36, 20, 1364, 110],
      );
    await finishPlotGlass(scene, { accent: "#BF9CFF" }, false, dimension);
    const violet = await capturePlotGlassFamilies(
      scene,
      capture,
      "violet-accent",
      dimension,
    );
    for (const [name, frame] of Object.entries(glass))
      check(
        plotPixelDifference(frame, violet[name]!).changed >
          (name.includes("point") ? 5 : 100),
        `${name}: palette parameter ignored`,
      );
    const afterView = await host.datasets.bindingView(
      scene.planar.client.session,
      scene.planar.charts[0]!.entity,
    );
    const afterSpatial = await plot3dView(scene.spatial, "grid-bars");
    check(
      afterView.evaluatedTick === beforeView.evaluatedTick &&
        afterSpatial.evaluatedTick === spatialView.evaluatedTick,
      "Material-only changes reevaluated source data",
    );
    await finishPlotGlass(scene, {}, false, dimension);
    if (dimension === 2) {
      await curvedMarkScales(scene, capture);
      await changePlot2d(scene.planar);
      await readyPlot2d(scene.planar);
      const deadline = performance.now() + 30_000;
      for (;;) {
        const chart = scene.planar.charts.find((item) =>
          item.source.endsWith("/bars"),
        )!;
        const page = await host.datasets.bindingView(
          scene.planar.client.session,
          chart.entity,
        );
        const value = page.rows.find((row) => row.id === 2n)?.values[
          page.columns.findIndex((column) => column.name === "value")
        ];
        if (
          !page.dirty &&
          value?.valid &&
          value.value.kind === "f32" &&
          value.value.value === 40
        )
          break;
        check(
          performance.now() < deadline,
          "Updated source cut did not reach glass bars",
        );
        await new Promise((resolve) => setTimeout(resolve, 10));
      }
      const changed = await capture("changed-data", scene.binding);
      check(
        plotPixelDifference(
          glass["2d-line-bars-pie"]!,
          changed,
          [130, 680, 350, 860],
        ).changed > 300,
        "Glass bars ignored real source edit",
      );
      await planarPicks(scene, true);
    } else {
      await changePlot3d(scene.spatial);
      const changed = await capturePlotGlassFamilies(
        scene,
        capture,
        "changed-data",
        3,
      );
      for (const name of [
        "3d-grid-bars",
        "3d-height-surface",
        "3d-point-plot",
        "3d-variable-pie",
      ])
        check(
          plotPixelDifference(glass[name]!, changed[name]!).changed > 50,
          `${name}: glass ignored real source edit`,
        );
    }
    if (dimension === 3)
      await record(
        "triangle-order",
        await assertPlotTriangleOrder(scene, capture),
      );
    await record("glass-responses", response);
    return {
      family,
      variants: Object.keys(glass),
      dataGeometry: true,
      nativePicking: true,
      light: true,
      palette: true,
    };
  } finally {
    await closePlotGlass(scene);
  }
}
