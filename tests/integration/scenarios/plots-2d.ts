/** Shared assertions over real prepared columns and completed backend frames. */
import {
  canvasOutput,
  type Client,
  type HostClientBase,
  type RootBinding,
  type PickingWorldClient,
  type GeometryPickResultEvent,
} from "@ipp/client";
import {
  PLOT_2D_EXTENT,
  openPlot2d,
  readyPlot2d,
  changePlot2d,
  closePlot2d,
  pickPlot2d,
  type PlotMarkPick,
  type Plot2dContract,
} from "../plot-2d-scene.js";
interface RgbaImage {
  width: number;
  height: number;
  pixels: Uint8Array;
}
function check(
  condition: unknown,
  message = "2D Plot invariant",
): asserts condition {
  if (!condition) throw new Error(message);
}
function equal(
  actual: unknown,
  expected: unknown,
  message = "2D Plot equality",
) {
  check(
    actual === expected,
    `${message}: ${String(actual)} != ${String(expected)}`,
  );
}
function cyanCount(
  frame: RgbaImage,
  box: readonly [number, number, number, number],
): number {
  let count = 0;
  for (let y = box[1]; y < box[3]; y++) {
    for (let x = box[0]; x < box[2]; x++) {
      const index = (y * frame.width + x) * 4;
      if (
        frame.pixels[index]! < 110 &&
        frame.pixels[index + 1]! > 170 &&
        frame.pixels[index + 2]! > 190
      )
        count++;
    }
  }
  return count;
}

function changedPixels(
  a: RgbaImage,
  b: RgbaImage,
  box: readonly [number, number, number, number],
): number {
  let count = 0;
  for (let y = box[1]; y < box[3]; y++) {
    for (let x = box[0]; x < box[2]; x++) {
      const index = (y * a.width + x) * 4;
      if (
        [0, 1, 2].some(
          (channel) =>
            Math.abs(a.pixels[index + channel]! - b.pixels[index + channel]!) >
            24,
        )
      )
        count++;
    }
  }
  return count;
}

export async function exercisePlot2d(
  host: HostClientBase<Client>,
  contract: Plot2dContract,
  font: Uint8Array<ArrayBuffer>,
  capture: (label: string, binding: RootBinding) => Promise<RgbaImage>,
  record: (label: string, value: unknown) => Promise<void> = async () => {},
) {
  const scene = await openPlot2d(host, contract, font, "integration");
  let binding: RootBinding | undefined;
  try {
    binding = await host.setRootOutput(
      canvasOutput(scene.world.reference),
      PLOT_2D_EXTENT,
    );
    await readyPlot2d(scene);
    const baseline = await capture("baseline", binding);
    // Bright series geometry is present in each panel; quiet grids cannot satisfy these thresholds.
    check(
      cyanCount(baseline, [105, 215, 642, 450]) > 700,
      "straight series absent",
    );
    check(
      cyanCount(baseline, [785, 215, 1320, 450]) > 700,
      "smooth series absent",
    );
    check(cyanCount(baseline, [150, 720, 310, 860]) > 4000, "bars absent");
    check(
      cyanCount(baseline, [1048, 680, 1140, 820]) > 2500,
      "pie sector absent",
    );
    // The invalid source sample at X=6 leaves a real gap between X=4 and X=8.
    check(
      cyanCount(baseline, [398, 315, 482, 414]) < 10,
      "invalid line sample was bridged",
    );
    const before = await host.datasets.bindingView(
      scene.client.session,
      scene.charts[0]!.entity,
    );
    equal(before.dirty, false);
    equal(before.rows[2]!.id, 3n);
    await record("binding-before", before);
    const baselinePicks = await pickPlot2d(scene, binding, false);
    const baselineLine = baselinePicks.line.ok
      ? (baselinePicks.line.hit as PlotMarkPick | null)
      : null;
    check(baselineLine, "baseline source mark must be pickable");
    equal(baselineLine.entity, scene.charts[0]!.entity);
    equal(baselineLine.component, scene.client.components.PlotLine2d!.id);
    check(
      baselineLine.row.series === 0 && baselineLine.row.rowId === 3n,
      "baseline source row identity",
    );
    for (const [name, rowId, component] of [
      ["bar", 2n, "PlotBars2d"],
      ["pie", 1n, "PlotPie2d"],
    ] as const) {
      const result = baselinePicks[name];
      check(
        result.ok &&
          result.hit?.row?.series === 0 &&
          result.hit.row.rowId === rowId,
        `${name} baseline source identity`,
      );
      equal(result.hit.component, scene.client.components[component]!.id);
    }
    await record("picks-before", baselinePicks);

    // Placement-only Canvas edits must move both paint and analytic row hits.
    // The independent fixture projection starts at (317,290) for source row 3.
    const lineEntity = scene.charts[0]!.entity;
    const style = scene.client.components.CanvasStyle!;
    const place = async (x: number, y: number) => {
      const outcome = await scene.client.batch([
        {
          kind: "setField",
          entity: { kind: "handle", id: lineEntity },
          component: style.id,
          field: {
            offset: style.fields.x!.offset,
            value: { kind: "f32", value: x },
          },
        },
        {
          kind: "setField",
          entity: { kind: "handle", id: lineEntity },
          component: style.id,
          field: {
            offset: style.fields.y!.offset,
            value: { kind: "f32", value: y },
          },
        },
      ]);
      check(outcome.ok, "Canvas placement edit failed");
    };
    await place(134, 213);
    const moved = await capture("placement-moved", binding);
    check(
      changedPixels(baseline, moved, [100, 210, 680, 490]) > 700,
      "Canvas placement did not move chart paint",
    );
    const picking = scene.client as unknown as PickingWorldClient;
    const movedBinding = binding;
    const pick = (x: number, y: number): Promise<GeometryPickResultEvent> =>
      picking.query({
        type: "GeometryPickQuery",
        view: { kind: "bound", binding: movedBinding },
        x: x / PLOT_2D_EXTENT.width,
        y: y / PLOT_2D_EXTENT.height,
        includeViewPlane: false,
      });
    const atMovedPoint = await pick(397, 325);
    check(
      atMovedPoint.ok &&
        atMovedPoint.hit?.entity === lineEntity &&
        atMovedPoint.hit.row?.series === 0 &&
        atMovedPoint.hit.row.rowId === 3n,
      "Canvas placement left row picking at its previous location",
    );
    const atOldPoint = await pick(317, 290);
    check(atOldPoint.ok, "Previous Canvas location query failed");
    const oldHit = atOldPoint.hit;
    check(
      !(oldHit?.entity === lineEntity && oldHit.row?.rowId === 3n),
      "Previous Canvas location retained a stale row hit",
    );
    const placed = await host.datasets.bindingView(
      scene.client.session,
      lineEntity,
    );
    equal(
      placed.evaluatedTick,
      before.evaluatedTick,
      "Placement reevaluated data",
    );
    equal(placed.dirty, false);
    await record("placement-picks", {
      atMovedPoint,
      atOldPoint,
      binding: placed,
    });
    await place(54, 178);
    await capture("placement-restored", binding);

    await changePlot2d(scene);
    await readyPlot2d(scene);
    // Dataset updates and World commands have separate admission. Await the
    // expected prepared cut before treating a dirty=false observation as current.
    const deadline = performance.now() + 30_000;
    for (;;) {
      const page = await host.datasets.bindingView(
        scene.client.session,
        scene.charts[1]!.entity,
      );
      const column = page.columns.findIndex((output) => output.name === "y2");
      const sample = page.rows.find((row) => row.id === 2n)?.values[column];
      if (
        !page.dirty &&
        sample?.valid &&
        sample.value.kind === "f32" &&
        Math.abs(sample.value.value - 21) < 1e-4
      )
        break;
      check(
        performance.now() < deadline,
        "Updated source/parameter cut did not reach Plot",
      );
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    const changed = await capture("changed", binding);
    check(
      changedPixels(baseline, changed, [100, 220, 645, 450]) > 700,
      "source edit did not update line geometry",
    );
    check(
      changedPixels(baseline, changed, [150, 700, 370, 875]) > 1500,
      "bar edit/highlight did not update geometry",
    );
    check(
      changedPixels(baseline, changed, [820, 650, 1300, 900]) > 400,
      "pie highlight did not change",
    );
    const after = await host.datasets.bindingView(
      scene.client.session,
      scene.charts[0]!.entity,
    );
    equal(
      after.rows[2]!.id,
      before.rows[2]!.id,
      "edit must retain source identity",
    );
    equal(
      after.dirty,
      false,
      "prepared geometry must acknowledge binding dirty",
    );
    await record("binding-after", after);
    const parameterPage = await host.datasets.bindingView(
      scene.client.session,
      scene.charts[1]!.entity,
    );
    const amberColumn = parameterPage.columns.findIndex(
      (column) => column.name === "y2",
    );
    const amber = parameterPage.rows[1]!.values[amberColumn]!;
    check(
      amber.valid && amber.value.kind === "f32",
      "parameter-driven projection must remain numeric",
    );
    check(
      Math.abs((amber.value.value as number) - 21) < 1e-4,
      "binding parameter must scale prepared samples",
    );
    await record("parameter-output", parameterPage);
    const changedPicks = await pickPlot2d(scene, binding, true);
    const changedLine = changedPicks.line.ok
      ? (changedPicks.line.hit as PlotMarkPick | null)
      : null;
    check(changedLine, "changed selected source mark must be pickable");
    check(
      changedLine.row.series === 0 && changedLine.row.rowId === 5n,
      "changed source row identity",
    );
    const selectedBar = changedPicks.bar.ok
      ? (changedPicks.bar.hit as PlotMarkPick | null)
      : null;
    const selectedPie = changedPicks.pie.ok
      ? (changedPicks.pie.hit as PlotMarkPick | null)
      : null;
    equal(selectedBar?.row.rowId, 4n);
    equal(selectedPie?.row.rowId, 2n);
    await record("picks-after", changedPicks);
    return {
      families: 3,
      gap: true,
      sourceIdentity: true,
      parameter: 21,
      rowPicks: 6,
      placementPicking: true,
    };
  } finally {
    if (binding) await host.clearRootOutput(binding);
    await closePlot2d(scene);
  }
}
