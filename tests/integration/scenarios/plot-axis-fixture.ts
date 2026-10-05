/** Shared physical-axis fixture and independent completed-frame observations. */
import type { Client } from "@ipp/client";
import type { Plot3dContract, Plot3dScene } from "../plot-3d-scene.js";
import type { PlotFrame } from "../plot-capture.js";
import {
  componentFields,
  successfulBatch,
} from "../../../examples/world-gallery/worlds/charts/shared/commands.js";

const DIMENSIONS = [10, 5, 10] as const;
const crossAxes = [
  [1, 2],
  [0, 2],
  [0, 1],
] as const;

export function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

export function yellow(frame: PlotFrame, x: number, y: number) {
  if (x < 0 || y < 0 || x >= frame.width || y >= frame.height) return false;
  const index = (y * frame.width + x) * 4;
  return (
    frame.pixels[index]! > 100 &&
    frame.pixels[index + 1]! > 100 &&
    Math.abs(frame.pixels[index]! - frame.pixels[index + 1]!) < 20 &&
    frame.pixels[index + 2]! < 80
  );
}

export function camera(
  center: readonly number[],
  yaw: number,
  pitch: number,
  height: number,
) {
  const eye = [
    center[0]! + 25 * Math.sin(yaw) * Math.cos(pitch),
    center[1]! - 25 * Math.sin(pitch),
    center[2]! + 25 * Math.cos(yaw) * Math.cos(pitch),
  ];
  const sy = Math.sin(yaw / 2),
    cy = Math.cos(yaw / 2),
    sx = Math.sin(pitch / 2),
    cx = Math.cos(pitch / 2);
  return {
    height,
    fields: {
      x: eye[0]!,
      y: eye[1]!,
      z: eye[2]!,
      qx: cy * sx,
      qy: sy * cx,
      qz: -sy * sx,
      qw: cy * cx,
    },
    project: (
      point: readonly number[],
      frame: Pick<PlotFrame, "width" | "height">,
    ) => {
      const delta = point.map((value, axis) => value - eye[axis]!);
      const right = Math.cos(yaw) * delta[0]! - Math.sin(yaw) * delta[2]!;
      const up =
        Math.sin(yaw) * Math.sin(pitch) * delta[0]! +
        Math.cos(pitch) * delta[1]! +
        Math.cos(yaw) * Math.sin(pitch) * delta[2]!;
      return [
        frame.width / 2 + (right * frame.height) / height,
        frame.height / 2 - (up * frame.height) / height,
      ];
    },
  };
}

export async function setFields(
  client: Client,
  entity: bigint,
  component: string,
  values: Record<string, number | string | boolean>,
) {
  successfulBatch(
    await client.batch(
      componentFields(client, component, values).map((field) => ({
        kind: "setField" as const,
        entity: { kind: "handle" as const, id: entity },
        component: client.components[component]!.id,
        field,
      })),
    ),
  );
}

export async function clearLabels(
  client: Client,
  entity: bigint,
  component: string,
  contract: Plot3dContract,
) {
  const labels = client.components[component]!.fields.labels!;
  check(labels.rows, "Motion fixture Plot labels contract unavailable");
  successfulBatch(
    await client.batch([
      {
        kind: "setField",
        entity: { kind: "handle", id: entity },
        component: client.components[component]!.id,
        field: {
          offset: labels.offset,
          value: {
            kind: "rows",
            value: contract.encodeRowsTable(labels.rows, {
              nextSlot: 0,
              rows: new Map(),
            }),
          },
        },
      },
    ]),
  );
}

export function axisInk(
  frame: PlotFrame,
  view: ReturnType<typeof camera>,
  axis: number,
  cross: readonly number[],
) {
  let visible = 0,
    ink = 0;
  for (let station = 0; station <= 60; station++) {
    const point = [0, 0, 0],
      others = crossAxes[axis]!;
    point[axis] = (DIMENSIONS[axis]! * station) / 60;
    point[others[0]] = cross[0]! * DIMENSIONS[others[0]];
    point[others[1]] = cross[1]! * DIMENSIONS[others[1]];
    const screen = view.project(point, frame),
      x = Math.round(screen[0]!),
      y = Math.round(screen[1]!);
    if (x < 3 || x >= frame.width - 3 || y < 3 || y >= frame.height - 3)
      continue;
    visible++;
    let found = false;
    for (let dy = -2; dy <= 2; dy++)
      for (let dx = -2; dx <= 2; dx++)
        if (yellow(frame, x + dx, y + dy)) found = true;
    if (found) ink++;
  }
  return { cross, visible, ink, fraction: visible >= 12 ? ink / visible : 0 };
}

export function perimeterInk(
  frame: PlotFrame,
  view: ReturnType<typeof camera>,
  axis: number,
) {
  const candidates = [];
  for (let step = 0; step <= 200; step++) {
    const t = step / 200;
    candidates.push(
      axisInk(frame, view, axis, [0, t]),
      axisInk(frame, view, axis, [1, t]),
      axisInk(frame, view, axis, [t, 0]),
      axisInk(frame, view, axis, [t, 1]),
    );
  }
  candidates.sort((a, b) => b.fraction - a.fraction || b.ink - a.ink);
  return candidates[0]!;
}

export function motionNumericInk(
  frame: PlotFrame,
  view: ReturnType<typeof camera>,
  axis: number,
  cross: readonly number[],
) {
  const others = crossAxes[axis]!,
    point = [0, 0, 0];
  point[others[0]] = cross[0]! * DIMENSIONS[others[0]];
  point[others[1]] = cross[1]! * DIMENSIONS[others[1]];
  const fontHeight = (0.27 * frame.height) / view.height;
  const start = view.project(point, frame);
  point[axis] = DIMENSIONS[axis]!;
  const end = view.project(point, frame),
    length = Math.hypot(end[0]! - start[0]!, end[1]! - start[1]!),
    tangent = [(end[0]! - start[0]!) / length, (end[1]! - start[1]!) / length];
  const station = [0.25, 0.5, 0.75]
    .map((fraction) => [
      start[0]! + fraction * (end[0]! - start[0]!),
      start[1]! + fraction * (end[1]! - start[1]!),
    ])
    .find((screen) =>
      screen.every(
        (value, index) =>
          value > fontHeight * 1.5 &&
          value < [frame.width, frame.height][index]! - fontHeight * 1.5,
      ),
    );
  check(
    station,
    "Intermediate axis retains an interior numeric station in its viewport",
  );
  let ink = 0;
  for (
    let y = Math.max(0, Math.floor(station[1]! - fontHeight * 4));
    y < Math.min(frame.height, station[1]! + fontHeight * 4);
    y++
  )
    for (
      let x = Math.max(0, Math.floor(station[0]! - fontHeight * 4));
      x < Math.min(frame.width, station[0]! + fontHeight * 4);
      x++
    ) {
      const dx = x - station[0]!,
        dy = y - station[1]!,
        along = dx * tangent[0]! + dy * tangent[1]!,
        normal = Math.abs(-dx * tangent[1]! + dy * tangent[0]!);
      if (
        Math.abs(along) < fontHeight * 1.5 &&
        normal > 5 &&
        normal < fontHeight * 3 &&
        yellow(frame, x, y)
      )
        ink++;
    }
  check(
    ink >= 10,
    `Intermediate numeric glyphs detached from their moving axis (${ink})`,
  );
  return { station, fontHeight, ink };
}

export async function prepareAxisPoints(scene: Plot3dScene) {
  const chart = scene.charts.find((item) => item.name === "point-plot")!;
  const fields = (
    entity: bigint,
    component: string,
    values: Record<string, number | string | boolean>,
  ) => setFields(chart.client, entity, component, values);
  const source = await scene.host.datasets.read(chart.source),
    y = source.schema.findIndex((column) => column.name === "y"),
    y2 = source.schema.findIndex((column) => column.name === "y2");
  check(
    y >= 0 && y2 >= 0 && source.rows.length === 12,
    "Sparse motion fixture reads its real source schema and rows",
  );
  const editHeight = async (height: number) => {
    const edit = await scene.host.datasets.update(
      scene.bars,
      source.rows.map((row) => ({
        operation: "edit" as const,
        row: row.id,
        values: row.values.map((value, index) =>
          index === y || index === y2
            ? {
                kind: "f32" as const,
                value: (row.id % 2n === 0n) === (index === y) ? height : 0,
              }
            : value,
        ),
      })),
    );
    check(!edit.failure, "Sparse enclosing point source failed to update");
  };
  await editHeight(100);
  await fields(chart.entity, "PlotFrame3d", {
    min_x: 0,
    max_x: 3,
    min_z: 0,
    max_z: 2,
    red: 1,
    green: 1,
    blue: 0,
    grid_alpha: 0,
    x_title: "X",
    y_title: "Y",
    z_title: "Z",
  });
  await clearLabels(
    chart.client,
    chart.entity,
    chart.component,
    scene.contract,
  );
  return { chart, fields, editHeight };
}
