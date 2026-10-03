/** Maintained real-runtime scenario; component-development execution is deferred. */
import type {
  Client,
  HostClientBase,
  RootBinding,
  DataBindingPage,
  PickingWorldClient,
} from "@ipp/client";
import {
  openPlot3d,
  readyPlot3d,
  rotatePlot3d,
  changePlot3d,
  plot3dView,
  closePlot3d,
  type Plot3dContract,
  type Plot3dScene,
} from "../plot-3d-scene.js";
import {
  componentFields,
  successfulBatch,
} from "../../../examples/chart-showcase/commands.js";

interface Frame {
  readonly width: number;
  readonly height: number;
  readonly pixels: Uint8Array;
}
type Capture = (label: string, binding: RootBinding) => Promise<Frame>;
const ORTHO_HEIGHT = 17;

function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function colored(frame: Frame, color: "cyan" | "amber" | "magenta"): number {
  let count = 0;
  for (let i = 0; i < frame.pixels.length; i += 4) {
    const r = frame.pixels[i]!,
      g = frame.pixels[i + 1]!,
      b = frame.pixels[i + 2]!;
    if (
      color === "cyan"
        ? r < 100 && g > 100 && b > 100
        : color === "amber"
          ? r > 150 && g > 80 && b < 120
          : r > 120 && b > 80 && g < 120
    )
      count++;
  }
  return count;
}

function difference(a: Frame, b: Frame): number {
  check(a.width === b.width && a.height === b.height, "Changed capture extent");
  let count = 0;
  for (let i = 0; i < a.pixels.length; i += 4) {
    if (
      Math.abs(a.pixels[i]! - b.pixels[i]!) +
        Math.abs(a.pixels[i + 1]! - b.pixels[i + 1]!) +
        Math.abs(a.pixels[i + 2]! - b.pixels[i + 2]!) >
      30
    )
      count++;
  }
  return count;
}

function number(view: DataBindingPage, row: bigint, name: string): number {
  const index = view.columns.findIndex((column) => column.name === name);
  check(index >= 0, `Missing ${name} output`);
  const value = view.rows.find((item) => item.id === row)?.values[index];
  check(
    value?.valid && value.value.kind === "f32",
    `Missing finite ${name} row ${row}`,
  );
  return value.value.value;
}

async function views(scene: Plot3dScene) {
  return new Map(
    await Promise.all(
      scene.charts.map(
        async (chart) =>
          [chart.name, await plot3dView(scene, chart.name)] as const,
      ),
    ),
  );
}

async function pickHighlightedBar(scene: Plot3dScene, rotated: boolean) {
  const chart = scene.charts.find((item) => item.name === "grid-bars")!;
  // Independent projection of source row 6's top centre through the fixture's
  // fixed linear axes and authored orthographic camera. No runtime geometry read.
  const eye = rotated ? [-10, 10, 14] : [10, 8, 14];
  const delta = [eye[0]! - 5, eye[1]! - 2, eye[2]! - 5];
  const yaw = Math.atan2(delta[0]!, delta[2]!);
  const pitch = -Math.atan2(delta[1]!, Math.hypot(delta[0]!, delta[2]!));
  const relative = [3.75 - eye[0]!, 4.25 - eye[1]!, 5 - eye[2]!];
  const right = Math.cos(yaw) * relative[0]! - Math.sin(yaw) * relative[2]!;
  const up =
    Math.sin(yaw) * Math.sin(pitch) * relative[0]! +
    Math.cos(pitch) * relative[1]! +
    Math.cos(yaw) * Math.sin(pitch) * relative[2]!;
  const client = chart.client as unknown as PickingWorldClient;
  const result = await client.query({
    type: "GeometryPickQuery",
    view: { kind: "bound", binding: chart.binding },
    x: 0.5 + right / ((ORTHO_HEIGHT * 960) / 760),
    y: 0.5 - up / ORTHO_HEIGHT,
    includeViewPlane: false,
  });
  check(result.ok && result.hit, "Highlighted 3D bar was not picked");
  check(result.hit.entity === chart.entity, "Picked a different chart entity");
  check(
    result.hit.component === chart.client.components.PlotGridBars3d!.id,
    "Wrong chart pick component",
  );
  check(
    result.hit.row?.series === 0 && result.hit.row.rowId === 6n,
    "3D source row identity was lost",
  );
}

/** Drivers own connection, process, graphics environment and completed-frame capture. */
export async function exercisePlot3d(
  host: HostClientBase<Client>,
  contract: Plot3dContract,
  font: Uint8Array<ArrayBuffer>,
  capture: Capture,
  name = "plot-3d-scenario",
) {
  const scene = await openPlot3d(host, contract, font, name);
  try {
    // One ordinary camera framing for all poses keeps the expanded back grid
    // and outward title lanes visible in this narrow standalone viewport.
    for (const chart of scene.charts)
      successfulBatch(
        await chart.client.batch(
          componentFields(chart.client, "Camera", {
            ortho_height: ORTHO_HEIGHT,
          }).map((field) => ({
            kind: "setField",
            entity: { kind: "handle", id: chart.camera },
            component: chart.client.components.Camera!.id,
            field,
          })),
        ),
      );
    await readyPlot3d(scene);
    const initial = await views(scene);
    const bars = initial.get("grid-bars")!,
      points = initial.get("point-plot")!,
      surface = initial.get("height-surface")!;
    check(
      bars.sourceIncarnation !== null &&
        bars.sourceIncarnation === points.sourceIncarnation,
      "Shared bar/point source was duplicated",
    );
    check(
      bars.rows.length === 12 && points.rows.length === 12,
      "Plot reduced the prepared sample set",
    );
    check(
      number(bars, 6n, "value") === 85 &&
        Math.abs(number(points, 6n, "y") - 3.4) < 1e-6,
      "Independent column projections did not prepare",
    );
    const y = surface.columns.findIndex((column) => column.name === "y");
    check(
      surface.rows.find((row) => row.id === 145n)?.values[y]?.valid === false,
      "Explicit surface hole disappeared",
    );
    const baseline = new Map<string, Frame>();
    for (const chart of scene.charts) {
      const frame = await capture(`baseline-${chart.name}`, chart.binding);
      check(
        colored(frame, "cyan") > 100,
        `${chart.name}: native data geometry absent`,
      );
      if (chart.name === "variable-pie")
        check(
          colored(frame, "amber") > 200 && colored(frame, "magenta") > 200,
          "Variable pie colours absent",
        );
      baseline.set(chart.name, frame);
    }
    await pickHighlightedBar(scene, false);
    await rotatePlot3d(scene, true);
    await readyPlot3d(scene);
    const cameraOnly = await views(scene);
    for (const chart of scene.charts) {
      const view = cameraOnly.get(chart.name)!;
      check(
        view.evaluatedTick === initial.get(chart.name)!.evaluatedTick &&
          !view.dirty,
        `${chart.name}: camera change reevaluated its binding`,
      );
      const frame = await capture(`rotated-${chart.name}`, chart.binding);
      check(
        difference(baseline.get(chart.name)!, frame) > 500,
        `${chart.name}: camera did not change actual geometry presentation`,
      );
    }
    await pickHighlightedBar(scene, true);
    await rotatePlot3d(scene, false);
    await changePlot3d(scene);
    await readyPlot3d(scene);
    // Updates and World commands are independent admissions; await the new
    // prepared source values rather than interpreting an update outcome as a tick.
    const deadline = performance.now() + 30_000;
    let changed = await views(scene);
    while (
      number(changed.get("grid-bars")!, 6n, "value") !== 35 ||
      number(changed.get("height-surface")!, 217n, "y") < 3.79
    ) {
      check(
        performance.now() < deadline,
        "Changed source cut did not reach Plot",
      );
      await new Promise((resolve) => setTimeout(resolve, 10));
      changed = await views(scene);
    }
    check(
      Math.abs(number(changed.get("point-plot")!, 6n, "y") - 1.4) < 1e-6,
      "Shared source edit failed independent projection",
    );
    check(
      number(changed.get("variable-pie")!, 1n, "value") === 20,
      "Pie shares did not update",
    );
    check(
      Math.abs(number(changed.get("variable-pie")!, 1n, "radius") - 2.2) <
        1e-6 &&
        Math.abs(number(changed.get("variable-pie")!, 1n, "height") - 3.1) <
          1e-6,
      "Pie dimensions were not independent prepared columns",
    );
    for (const chart of scene.charts.filter(
      (chart) => chart.name !== "single-row",
    )) {
      const frame = await capture(`changed-${chart.name}`, chart.binding);
      check(
        difference(baseline.get(chart.name)!, frame) > 100,
        `${chart.name}: source edit did not change native pixels`,
      );
    }
    return {
      families: 4,
      singleRow: true,
      sharedSourceRows: 12,
      explicitHole: 145n,
      irregularSurfaceEdit: 217n,
      cameraChanges: 5,
      exactRowPicks: 2,
    };
  } finally {
    await closePlot3d(scene);
  }
}
