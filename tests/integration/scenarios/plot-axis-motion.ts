/** Real Host-clock axis motion, perimeter placement and shallow-row readability. */
import type { Client, HostClientBase, PickingWorldClient } from "@ipp/client";
import {
  closePlot3d,
  openPlot3d,
  plot3dView,
  readyPlot3d,
  type Plot3dContract,
} from "../plot-3d-scene.js";
import type { PlotCapture, PlotFrame } from "../plot-capture.js";
import {
  axisInk,
  camera,
  check,
  clearLabels,
  motionNumericInk,
  prepareAxisPoints,
  setFields,
  yellow,
} from "./plot-axis-fixture.js";

const EXTENT = { width: 480, height: 480, devicePixelRatio: 1 };
const SHALLOW_EXTENT = { width: 760, height: 760, devicePixelRatio: 1 };
function shallowNumericInk(frame: PlotFrame, view: ReturnType<typeof camera>) {
  const a = view.project([0, 5, 0], frame),
    b = view.project([0, 5, 2], frame),
    middle = view.project([5, 2.5, 1], frame);
  const length = Math.hypot(b[0]! - a[0]!, b[1]! - a[1]!),
    tangent = [(b[0]! - a[0]!) / length, (b[1]! - a[1]!) / length];
  let normal = [-tangent[1]!, tangent[0]!];
  if ((a[0]! - middle[0]!) * normal[0]! + (a[1]! - middle[1]!) * normal[1]! < 0)
    normal = normal.map((value) => -value);
  const fontHeight = (0.27 * frame.height) / view.height,
    histogram = new Map<number, number>();
  let startInk = 0,
    endInk = 0;
  for (let y = 0; y < frame.height; y++)
    for (let x = 0; x < frame.width; x++) {
      if (!yellow(frame, x, y)) continue;
      const dx = x - a[0]!,
        dy = y - a[1]!,
        along = dx * tangent[0]! + dy * tangent[1]!,
        outward = dx * normal[0]! + dy * normal[1]!;
      if (
        outward < 3 ||
        outward > fontHeight * 3 ||
        along < -fontHeight ||
        along > length + fontHeight
      )
        continue;
      const bin = Math.round(along);
      histogram.set(bin, (histogram.get(bin) ?? 0) + 1);
      if (Math.abs(along) < fontHeight * 0.65) startInk++;
      if (Math.abs(along - length) < fontHeight * 0.65) endInk++;
    }
  const runs: { min: number; max: number; ink: number }[] = [];
  for (const [bin, ink] of [...histogram].sort((a, b) => a[0] - b[0])) {
    const last = runs.at(-1);
    if (last && bin <= last.max + 2) {
      last.max = bin;
      last.ink += ink;
    } else runs.push({ min: bin, max: bin, ink });
  }
  check(
    startInk >= 5 && endInk >= 5,
    `Shallow Z axis lost numeric glyph ink near its ends (${startInk},${endInk})`,
  );
  check(
    runs.length >= 2 && runs.length <= 8,
    `Shallow32-tick Z axis must retain separated readable numeric groups (${JSON.stringify(runs)})`,
  );
  return { a, b, length, fontHeight, startInk, endInk, runs };
}

/** Observe a continuous axis segment independently of renderer-selected placement. */
function boundaryInk(
  frame: PlotFrame,
  view: ReturnType<typeof camera>,
  axis: number,
  low: number,
  high: number,
) {
  const candidates = [];
  // Opposite corners must pass through one of the two adjacent corners. This
  // samples the enclosure faces themselves, rather than production route code.
  for (let step = 0; step <= 100; step++) {
    const t = low + ((high - low) * step) / 100;
    candidates.push(
      axisInk(frame, view, axis, [0, 1 - t]),
      axisInk(frame, view, axis, [t, 1]),
    );
  }
  candidates.sort((a, b) => b.fraction - a.fraction || b.ink - a.ink);
  return candidates[0]!;
}

function cyanPoint(frame: PlotFrame, view: ReturnType<typeof camera>) {
  // Source row6 is x1,z1,y100. The point fixture scales y by25, then maps
  // fixed0..4 data height into the five-metre enclosure.
  const screen = view.project([10 / 3, 5, 5], frame),
    x = Math.round(screen[0]!),
    y = Math.round(screen[1]!);
  let pixels = 0;
  for (let py = y - 2; py <= y + 2; py++)
    for (let px = x - 2; px <= x + 2; px++) {
      const at = (py * frame.width + px) * 4;
      // View-dependent lighting can darken the lower facet to [0,133,147].
      // Keep a resolved cyan hue/intensity oracle that excludes the yellow
      // axis, amber series and background, while source RGBA stays unchanged.
      const red = frame.pixels[at]!,
        green = frame.pixels[at + 1]!,
        blue = frame.pixels[at + 2]!;
      if (
        green > 100 &&
        blue > 100 &&
        red < Math.min(green, blue) * 0.4 &&
        Math.abs(green - blue) < 25
      )
        pixels++;
    }
  check(
    pixels >= 5,
    "Axis motion moved or recolored the independently projected cyan source point",
  );
  return { screen, pixels };
}

export async function exercisePlotAxisMotion(
  host: HostClientBase<Client>,
  contract: Plot3dContract,
  font: Uint8Array<ArrayBuffer>,
  capture: PlotCapture,
  record: (label: string, value: unknown) => Promise<void>,
) {
  const scene = await openPlot3d(host, contract, font, "plot-axis-motion");
  try {
    const { chart, fields } = await prepareAxisPoints(scene);
    await fields(chart.entity, "PlotFrame3d", { adaptive_axes: true });
    await readyPlot3d(scene);
    const original = await plot3dView(scene, chart.name),
      binding = await host.setRootOutput(chart.binding.output, EXTENT);
    // A 10 m view keeps a quarter tick at least 35px inside the 480px viewport
    // over either complete X/Z boundary route; the numeric glyph is 13px high.
    const cases = [
      {
        axis: 0,
        initial: camera([5, 0, 10], 0, -0.4, 10),
        target: camera([5, 5, 0], 2.7, -0.4, 10),
      },
      {
        axis: 1,
        initial: camera([0, 2.5, 10], -Math.PI / 4, -0.4, 12),
        target: camera([10, 2.5, 0], (3 * Math.PI) / 4, -0.4, 12),
      },
      {
        axis: 2,
        initial: camera([0, 5, 5], -1.2, -0.4, 10),
        target: camera([10, 0, 5], 1.2, 0.4, 10),
      },
    ];
    for (const item of cases) {
      const label = `axis-motion-${"XYZ"[item.axis]}`;
      await fields(chart.camera, "Camera", {
        ortho_height: item.initial.height,
      });
      await fields(chart.camera, "Transform", item.initial.fields);
      const initial = await capture.afterMotion(
        `${label}-reference`,
        binding,
        chart.client,
      );
      const standard = axisInk(initial, item.initial, item.axis, [0, 1]);
      check(
        standard.fraction >= 0.75,
        `${label}: visible standard edge is not preferred`,
      );
      const samples = await capture.motion(
        label,
        binding,
        chart.client,
        () => fields(chart.camera, "Transform", item.target.fields),
        0.5,
      );
      const elapsed = {
        middleLeast: samples.middle.before.time - samples.origin.after.time,
        middleMost: samples.middle.after.time - samples.origin.before.time,
        endLeast: samples.end.before.time - samples.origin.after.time,
      };
      check(
        samples.start.sequence < samples.middle.sequence &&
          samples.middle.sequence < samples.end.sequence &&
          elapsed.middleLeast + 0.000001 >= 0.5 &&
          elapsed.middleMost < 1.25 &&
          elapsed.endLeast + 0.000001 >= 2,
        `${label}: completed samples do not bracket intermediate and settled2s motion on the Host clock`,
      );
      const first = boundaryInk(
          samples.start.frame,
          item.target,
          item.axis,
          0,
          0.3,
        ),
        intermediate = boundaryInk(
          samples.middle.frame,
          item.target,
          item.axis,
          0.15,
          0.85,
        ),
        end = axisInk(samples.end.frame, item.target, item.axis, [1, 0]);
      check(
        first.fraction >= 0.7 &&
          intermediate.fraction >= 0.7 &&
          end.fraction >= 0.75,
        `${label}: visible axis did not move along enclosure faces to its clear opposite edge (${JSON.stringify({ first, intermediate, end })})`,
      );
      check(
        intermediate.cross[0]! > 0.1 || intermediate.cross[1]! < 0.9,
        `${label}: intermediate axis remained at its departure corner`,
      );
      const direct = Array.from({ length: 31 }, (_, index) => {
        const t = 0.15 + (index * 0.7) / 30;
        return axisInk(samples.middle.frame, item.target, item.axis, [
          t,
          1 - t,
        ]);
      }).sort((a, b) => b.fraction - a.fraction)[0]!;
      check(
        direct.fraction < 0.4,
        `${label}: axis crossed the enclosure interior instead of its boundary (${JSON.stringify(direct)})`,
      );
      const glyphs = motionNumericInk(
        samples.middle.frame,
        item.target,
        item.axis,
        intermediate.cross,
      );
      const geometry = [samples.start, samples.middle, samples.end].map(
        (sample) => cyanPoint(sample.frame, item.target),
      );
      const pick = await (chart.client as unknown as PickingWorldClient).query({
        type: "GeometryPickQuery",
        view: { kind: "bound", binding },
        x: geometry[2]!.screen[0]! / EXTENT.width,
        y: geometry[2]!.screen[1]! / EXTENT.height,
        includeViewPlane: false,
      });
      check(
        pick.ok &&
          pick.hit?.entity === chart.entity &&
          pick.hit.component === chart.client.components.PlotPoints3d!.id &&
          pick.hit.row?.series === 0 &&
          pick.hit.row.rowId === 6n,
        `${label}: axis presentation changed exact source-row point picking`,
      );
      const current = await plot3dView(scene, chart.name);
      const json = (value: unknown) =>
        JSON.stringify(value, (_key, value) =>
          typeof value === "bigint" ? String(value) : value,
        );
      check(
        current.sourceIncarnation === original.sourceIncarnation &&
          current.bindingIncarnation === original.bindingIncarnation &&
          current.evaluatedTick === original.evaluatedTick &&
          !current.dirty &&
          json(current.rows) === json(original.rows) &&
          json(current.columns) === json(original.columns),
        `${label}: presentation-only motion rebuilt source values or mark geometry`,
      );
      await record(label, {
        axis: item.axis,
        elapsed,
        standard,
        first,
        intermediate,
        end,
        direct,
        glyphs,
        geometry,
        pick,
        source: current.sourceIncarnation,
        binding: current.bindingIncarnation,
        evaluatedTick: current.evaluatedTick,
      });
    }
    const single = scene.charts.find((item) => item.name === "single-row")!;
    await setFields(single.client, single.entity, "PlotFrame3d", {
      adaptive_axes: true,
      depth: 2,
      min_z: -1,
      max_z: 1,
      ticks: 32,
      red: 1,
      green: 1,
      blue: 0,
      grid_alpha: 0,
      x_title: "",
      y_title: "",
      z_title: "",
    });
    await clearLabels(single.client, single.entity, single.component, contract);
    const shallowView = camera([5, 2.5, 1], 0.8, -0.4, 17);
    await setFields(single.client, single.camera, "Camera", {
      ortho_height: shallowView.height,
    });
    await setFields(
      single.client,
      single.camera,
      "Transform",
      shallowView.fields,
    );
    const singleBinding = await host.setRootOutput(
      single.binding.output,
      SHALLOW_EXTENT,
    );
    const shallow = await capture.afterMotion(
      "axis-shallow-row-thinned",
      singleBinding,
      single.client,
    );
    const numeric = shallowNumericInk(shallow, shallowView);
    const inspect = await single.client.inspect(),
      entity = inspect.entities.find((entity) => entity.id === single.entity)!;
    const frame = entity.components.find(
        (component) => "min_z" in component.fields,
      )!.fields,
      bars = entity.components.find(
        (component) => "bar_depth" in component.fields,
      )!.fields;
    check(
      frame.depth === 2 &&
        frame.ticks === 32 &&
        frame.min_z === -1 &&
        frame.max_z === 1 &&
        bars.bar_depth === 1.5 &&
        (Number(frame.depth) - Number(bars.bar_depth)) / 2 === 0.25,
      "Shallow axis enclosure retains the authored single-row bar depth and margins",
    );
    const point = shallowView.project([3.75, 2.5, 1], shallow),
      pick = await (single.client as unknown as PickingWorldClient).query({
        type: "GeometryPickQuery",
        view: { kind: "bound", binding: singleBinding },
        x: point[0]! / shallow.width,
        y: point[1]! / shallow.height,
        includeViewPlane: false,
      });
    check(
      pick.ok &&
        pick.hit?.entity === single.entity &&
        pick.hit.row?.series === 0 &&
        pick.hit.row.rowId === 2n,
      "Numeric thinning changed exact shallow-row bar picking",
    );
    await record("axis-shallow-row-thinned", { numeric, frame, bars, pick });
    return {
      axes: 3,
      hostDuration: 2,
      geometryRetained: true,
      sourceRowPicks: 4,
      shallowTicks: 32,
    };
  } finally {
    await closePlot3d(scene);
  }
}
