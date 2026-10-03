/** Real-source 3D Plot fixture shared by native visual iteration and future scenarios. */
import type {
  AnimationWorldClient,
  Client,
  DatasetProducer,
  DatasetValue,
  HostClientBase,
  FieldWrite,
  RootBinding,
  RowPropertyValue,
  DataBindingPage,
} from "@ipp/client";
import { clientAssetSource } from "@ipp/client";
import type * as Generated from "@ipp/host-contract";
import {
  aliasId,
  componentFields,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../charts/shared/commands.js";

export type Plot3dContract = PlotContract &
  Pick<
    typeof Generated,
    "ExpressionBuilder" | "encodeRowsTable" | "GUI_SKIN_TOKENS"
  >;
export const PLOT_3D_EXTENT = {
  width: 960,
  height: 760,
  devicePixelRatio: 1,
} as const;
const CYAN = [0.0, 0.8, 1.0, 1.0] as const;
const SCHEMA = [
  { name: "x", kind: "f32" },
  { name: "y", kind: "f32" },
  { name: "z", kind: "f32" },
  { name: "y2", kind: "f32" },
  { name: "valid", kind: "f32" },
  { name: "radius", kind: "f32" },
  { name: "height", kind: "f32" },
  { name: "color", kind: "vec4" },
] as const;
import { declarePlot } from "../charts/shared/declare-plot.js";
import type { ReactWorldRoot, PlotContract } from "@ipp/react";

type Row = Readonly<Record<string, RowPropertyValue>>;
type Fields = Record<
  string,
  number | boolean | string | Uint8Array<ArrayBuffer>
>;

function data(
  x: number,
  y: number,
  z: number,
  y2 = y,
  valid = 1,
  radius = 1,
  height = 0.5,
  color: readonly number[] = CYAN,
): DatasetValue[] {
  const values: DatasetValue[] = [x, y, z, y2, valid, radius, height].map(
    (value) => ({ kind: "f32", value }),
  );
  values.push({ kind: "vec4", value: [...color] });
  return values;
}

function series(
  color: readonly number[] = CYAN,
  y = "y",
  z = "z",
  variable = false,
): Row {
  return {
    name: y === "y2" ? "Group B" : "Group A",
    x: "x",
    y,
    z,
    value: "value",
    radius: variable ? "radius" : "",
    height: variable ? "height" : "",
    color_column: variable ? "color" : "",
    color,
    visible: true,
  };
}

function label(
  row: bigint,
  text: string,
  offset: readonly number[],
  highlighted = false,
  slot = 0,
): Row {
  return {
    series: slot,
    row_id: row.toString(),
    text,
    offset,
    highlighted,
    connector: true,
  };
}

interface Plot3dChart {
  readonly name: string;
  readonly world: Awaited<ReturnType<HostClientBase<Client>["createWorld"]>>;
  readonly client: AnimationWorldClient;
  readonly entity: bigint;
  readonly camera: bigint;
  readonly binding: RootBinding;
  readonly source: string;
  readonly font: string;
  readonly component: string;
  readonly root: ReactWorldRoot;
}

export interface Plot3dScene {
  readonly host: HostClientBase<Client>;
  readonly contract: Plot3dContract;
  readonly producers: DatasetProducer[];
  readonly charts: Plot3dChart[];
  readonly bars: DatasetProducer;
  readonly pie: DatasetProducer;
  readonly surface: DatasetProducer;
}

function rows(
  client: Client,
  contract: Plot3dContract,
  component: string,
  field: string,
  values: readonly Row[],
) {
  const layout = client.components[component]?.fields[field]?.rows;
  if (!layout)
    throw new Error(`Missing generated ${component}.${field} layout`);
  return contract.encodeRowsTable(layout, {
    nextSlot: values.length,
    rows: new Map(values.map((value, index) => [index, value])),
  });
}

function rowField(
  client: Client,
  contract: Plot3dContract,
  component: string,
  field: string,
  values: readonly Row[],
): FieldWrite {
  return {
    offset: client.components[component]!.fields[field]!.offset,
    value: {
      kind: "rows",
      value: rows(client, contract, component, field, values),
    },
  };
}

function cameraPose(rotated: boolean): Record<string, number> {
  const eye = rotated ? [-10, 10, 14] : [10, 8, 14];
  const target = [5, 2.0, 5];
  const direction = eye.map((value, axis) => value - target[axis]!);
  const yaw = Math.atan2(direction[0]!, direction[2]!);
  const pitch = -Math.atan2(
    direction[1]!,
    Math.hypot(direction[0]!, direction[2]!),
  );
  const sy = Math.sin(yaw / 2),
    cy = Math.cos(yaw / 2),
    sx = Math.sin(pitch / 2),
    cx = Math.cos(pitch / 2);
  return {
    x: eye[0]!,
    y: eye[1]!,
    z: eye[2]!,
    qx: cy * sx,
    qy: sy * cx,
    qz: -sy * sx,
    qw: cy * cx,
  };
}

/** Hosts own ticks; the client supplies only sources, selectors and presentation. */
export async function openPlot3d(
  host: HostClientBase<Client>,
  contract: Plot3dContract,
  font: Uint8Array<ArrayBuffer>,
  name: string,
): Promise<Plot3dScene> {
  const {
    accent: CYAN,
    amber: AMBER,
    error: MAGENTA,
    neutral: NEUTRAL,
  } = contract.GUI_SKIN_TOKENS;
  const producers: DatasetProducer[] = [];
  const charts: Plot3dChart[] = [];
  const source = async (key: string, values: DatasetValue[][]) => {
    const source = `datasets://plots-3d/${name}/${key}`;
    const producer = await host.datasets.create(source, "buffer", SCHEMA);
    producers.push(producer);
    const result = await host.datasets.update(producer, [
      { operation: "append", rows: values },
    ]);
    if (result.failure)
      throw new Error(`Fixture ingestion: ${result.failure.reason}`);
    return { producer, source };
  };
  const barRows = [35, 75, 45, 25, 60, 85, 50, 30, 55, 70, 40, 20].map(
    (value, i) => data(i % 4, value, Math.floor(i / 4), 15 + ((i * 17) % 75)),
  );
  const bars = await source("bars-and-points", barRows);
  const single = await source(
    "single-row",
    [30, 50, 35, 45].map((value, i) => data(i, value, 0)),
  );
  const surfaceRows: DatasetValue[][] = [];
  for (let x = 0; x <= 16; x++)
    for (let z = 0; z <= 16; z++) {
      const px = (x * 10) / 16,
        pz = (z * 10) / 16;
      const y =
        0.3 +
        2.5 * Math.exp(-((px - 3) ** 2 + (pz - 4) ** 2) / 5) +
        2.8 * Math.exp(-((px - 7.5) ** 2 + (pz - 7.5) ** 2) / 4);
      surfaceRows.push(data(px, y, pz, y, x === 8 && z === 8 ? 0 : 1));
    }
  const surface = await source("height", surfaceRows);
  const pie = await source("pie", [
    data(0, 40, 0, 40, 1, 3.0, 2.4, CYAN),
    data(0, 30, 0, 30, 1, 2.4, 1.5, NEUTRAL),
    data(0, 20, 0, 20, 1, 2.1, 0.9, AMBER),
    data(0, 10, 0, 10, 1, 2.7, 1.8, MAGENTA),
  ]);
  const scene: Plot3dScene = {
    host,
    contract,
    producers,
    charts,
    bars: bars.producer,
    pie: pie.producer,
    surface: surface.producer,
  };

  const add = async (
    key: string,
    component: string,
    producer: { producer: DatasetProducer; source: string },
    seriesRows: readonly Row[],
    labelRows: readonly Row[],
    frame: Fields = {},
    extra: Fields = {},
    pointScale = false,
  ) => {
    const world = await host.createWorld({
      symbolicId: `plots-3d/${name}/${key}`,
      temporary: true,
      selectedSystems: [
        "ipp.hierarchy",
        "ipp.look-at",
        "ipp.final-propagation",
        "ipp.animation",
        "ipp.asset-dependencies",
        "ipp.data-bindings",
        "ipp.plot",
        "ipp.geometry",
        "ipp.camera",
        "ipp.render",
      ],
    });
    const client = (await host.openWorld(
      world.reference,
    )) as AnimationWorldClient;
    const fontAsset = clientAssetSource(client.session, 17, 1n);
    await client.registerAsset(fontAsset, font.buffer);
    const definitions: Record<string, Uint8Array<ArrayBuffer>> = {};
    for (const output of [
      "x",
      "y",
      "z",
      "y2",
      "value",
      "radius",
      "height",
      "color",
    ]) {
      const builder = new contract.ExpressionBuilder();
      const raw = output === "value" ? "y" : output;
      const input = builder.input(
        `column:${raw}`,
        output === "color" ? "vec4" : "f32",
      );
      let result = input;
      if (output === "y" || output === "y2") {
        result = builder.binary(
          "divide",
          result,
          builder.input("column:valid", "f32"),
        );
        if (pointScale)
          result = builder.binary(
            "divide",
            result,
            builder.constant({ kind: "f32", value: 25 }),
          );
      }
      definitions[output] = builder.encode(result);
    }
    const ref = { kind: "alias" as const, alias: 1 };
    const cameraRef = { kind: "alias" as const, alias: 2 };
    const outcome = await client.batch([
      createEntity(1, key),
      insertComponent(client, "Transform", ref),
      insertComponent(client, "PlotFrame3d", ref, {
        width: 10,
        height: 5,
        depth: 10,
        min_x: -0.5,
        max_x: 3.5,
        min_y: 0,
        max_y: pointScale ? 4 : 100,
        min_z: -0.5,
        max_z: 2.5,
        automatic_x: false,
        automatic_y: false,
        automatic_z: false,
        ticks: 4,
        source: fontAsset.source,
        font_size: 0.27,
        x_title: "POSITION X",
        y_title: "HEIGHT Y",
        z_title: "POSITION Z",
        ...frame,
      }),
      createEntity(2, "camera"),
      insertComponent(client, "Transform", cameraRef, cameraPose(false)),
      insertComponent(client, "Camera", cameraRef, {
        projection: 1,
        ortho_height: 14.5,
        near: 0.1,
        far: 100,
      }),
    ]);
    const entity = aliasId(outcome, 1),
      camera = aliasId(outcome, 2);
    const root = await declarePlot(
      client,
      contract,
      key,
      component as Parameters<typeof declarePlot>[3],
      producer.source,
      definitions,
      seriesRows,
      labelRows,
      extra,
    );
    const output = await host.bindOutput(world.reference, camera, "camera");
    const binding = await host.setRootOutput(output, PLOT_3D_EXTENT);
    charts.push({
      root,
      name: key,
      world,
      client,
      entity,
      camera,
      binding,
      source: producer.source,
      font: fontAsset.source,
      component,
    });
  };
  try {
    await add(
      "grid-bars",
      "PlotGridBars3d",
      bars,
      [series(CYAN)],
      [label(6n, "B / 85 UNITS", [1, -2.4], true)],
      { y_title: "OUTPUT / UNITS", z_title: "BATCH Z" },
      { bar_width: 1.3, bar_depth: 1.5 },
    );
    await add(
      "single-row",
      "PlotGridBars3d",
      single,
      [series(CYAN, "y", "")],
      [label(2n, "SINGLE ROW / B", [1, -1], true)],
      { y_title: "OUTPUT / UNITS" },
      { bar_width: 1.3, bar_depth: 1.5 },
    );
    await add(
      "height-surface",
      "PlotHeightSurface3d",
      surface,
      [series(CYAN)],
      [label(217n, "PEAK / Y 3.1 M", [1.2, -1.8], true)],
      {
        min_x: 0,
        max_x: 10,
        max_y: 4,
        min_z: 0,
        max_z: 10,
        y_title: "HEIGHT Y / M",
        x_title: "POSITION X / M",
        z_title: "POSITION Z / M",
      },
      { wireframe: true, line_width: 0.018 },
    );
    await add(
      "point-plot",
      "PlotPoints3d",
      bars,
      [series(), series(AMBER, "y2")],
      [label(7n, "SAMPLE / GROUP A", [1, -1.5], true)],
      { y_title: "Y / M", x_title: "X / M", z_title: "Z / M" },
      { marker_size: 0.23, marker_shape: 1 },
      true,
    );
    await add(
      "variable-pie",
      "PlotPie3d",
      pie,
      [series(CYAN, "y", "z", true)],
      [
        label(1n, "A 40% / R3 H2.4", [0, 0], true),
        label(2n, "B 30% / R2.4 H1.5", [0, 0]),
        label(3n, "C 20% / R2.1 H0.9", [0, 0]),
        label(4n, "D 10% / R2.7 H1.8", [0, 0]),
      ],
      {},
      { start_angle: Math.PI / 2 },
    );
    return scene;
  } catch (error) {
    await closePlot3d(scene);
    throw error;
  }
}

export async function readyPlot3d(scene: Plot3dScene): Promise<void> {
  for (const chart of scene.charts) {
    const deadline = performance.now() + 30_000;
    for (;;) {
      const view = await scene.host.datasets.bindingView(
        chart.client.session,
        chart.entity,
      );
      const resources = (await chart.client.inspect()).resources.filter(
        (resource) => resource.source === chart.font,
      );
      if (resources.some((resource) => resource.status === "failed"))
        throw new Error("Plot font failed");
      if (
        view.availability.reason === "Ready" &&
        !view.dirty &&
        resources.some((resource) => resource.status === "loaded")
      )
        break;
      if (performance.now() > deadline)
        throw new Error(
          `Plot preparation timeout: ${chart.name}/${JSON.stringify(view.availability)}/dirty=${view.dirty}`,
        );
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
  }
}

/** Collect the complete read-only fixture view through the protocol page limit. */
export async function plot3dView(
  scene: Plot3dScene,
  name: string,
): Promise<DataBindingPage> {
  const chart = scene.charts.find((item) => item.name === name)!;
  const first = await scene.host.datasets.bindingView(
    chart.client.session,
    chart.entity,
  );
  const rows = [...first.rows];
  let next = first.nextOffset;
  while (next !== null) {
    const page = await scene.host.datasets.bindingView(
      chart.client.session,
      chart.entity,
      { offset: next },
    );
    rows.push(...page.rows);
    next = page.nextOffset;
  }
  return { ...first, rows, nextOffset: null };
}

export async function rotatePlot3d(
  scene: Plot3dScene,
  rotated: boolean,
): Promise<void> {
  for (const chart of scene.charts)
    successfulBatch(
      await chart.client.batch(
        componentFields(chart.client, "Transform", cameraPose(rotated)).map(
          (field) => ({
            kind: "setField",
            entity: { kind: "handle", id: chart.camera },
            component: chart.client.components.Transform!.id,
            field,
          }),
        ),
      ),
    );
}

export async function changePlot3d(
  scene: Plot3dScene,
  changed = true,
): Promise<void> {
  const peak =
    0.3 + 2.5 * Math.exp(-((7.5 - 3) ** 2 + (7.5 - 4) ** 2) / 5) + 2.8;
  const edits = await scene.host.datasets.update(scene.bars, [
    {
      operation: "edit",
      row: 6n,
      values: changed ? data(1, 35, 1, 80) : data(1, 85, 1, 25),
    },
  ]);
  if (edits.failure) throw new Error("Bar/point source edit failed");
  const pie = await scene.host.datasets.update(scene.pie, [
    {
      operation: "edit",
      row: 1n,
      values: changed
        ? data(0, 20, 0, 20, 1, 2.2, 3.1, CYAN)
        : data(0, 40, 0, 40, 1, 3.0, 2.4, CYAN),
    },
  ]);
  if (pie.failure) throw new Error("Variable pie source edit failed");
  const terrain = await scene.host.datasets.update(scene.surface, [
    {
      operation: "edit",
      row: 217n,
      values: changed ? data(7.6, 3.8, 7.4, 3.8) : data(7.5, peak, 7.5),
    },
  ]);
  if (terrain.failure) throw new Error("Irregular height sample edit failed");
  for (const chart of scene.charts) {
    const labels =
      chart.name === "grid-bars"
        ? [
            label(
              6n,
              changed ? "B / 35 UNITS" : "B / 85 UNITS",
              [1, -2.4],
              true,
            ),
          ]
        : chart.name === "variable-pie"
          ? [
              label(
                1n,
                changed ? "A 25% / R2.2 H3.1" : "A 40% / R3 H2.4",
                [0, 0],
                true,
              ),
              label(
                2n,
                changed ? "B 37.5% / R2.4 H1.5" : "B 30% / R2.4 H1.5",
                [0, 0],
              ),
              label(
                3n,
                changed ? "C 25% / R2.1 H0.9" : "C 20% / R2.1 H0.9",
                [0, 0],
              ),
              label(
                4n,
                changed ? "D 12.5% / R2.7 H1.8" : "D 10% / R2.7 H1.8",
                [0, 0],
              ),
            ]
          : chart.name === "height-surface"
            ? [
                label(
                  217n,
                  changed ? "PEAK / Y 3.8 M" : "PEAK / Y 3.1 M",
                  [1.2, -1.8],
                  true,
                ),
              ]
            : undefined;
    if (labels)
      successfulBatch(
        await chart.client.batch([
          {
            kind: "setField",
            entity: { kind: "handle", id: chart.entity },
            component: chart.client.components[chart.component]!.id,
            field: rowField(
              chart.client,
              scene.contract,
              chart.component,
              "labels",
              labels,
            ),
          },
        ]),
      );
  }

  // Producer updates and World commands have independent admissions. Wait for
  // the prepared source cut rather than treating an update outcome as a frame.
  const deadline = performance.now() + 30_000;
  for (;;) {
    const expected = [
      ["grid-bars", 6n, "value", changed ? 35 : 85],
      ["point-plot", 6n, "y", changed ? 1.4 : 3.4],
      ["height-surface", 217n, "y", changed ? 3.8 : peak],
      ["variable-pie", 1n, "value", changed ? 20 : 40],
    ] as const;
    const prepared = await Promise.all(
      expected.map(async ([name, row, output, value]) => {
        const view = await plot3dView(scene, name);
        const column = view.columns.findIndex((item) => item.name === output);
        const cell = view.rows.find((item) => item.id === row)?.values[column];
        return (
          !view.dirty &&
          cell?.valid &&
          cell.value.kind === "f32" &&
          Math.abs(cell.value.value - value) < 1e-5
        );
      }),
    );
    if (prepared.every(Boolean)) break;
    if (performance.now() > deadline)
      throw new Error("Changed source cut did not reach Plot");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

export async function closePlot3d(scene: Plot3dScene): Promise<void> {
  for (const chart of scene.charts) {
    await chart.root.unmount();
    await chart.client.close();
    await scene.host.destroyWorld(chart.world.reference);
  }
  await Promise.allSettled(
    scene.producers.map((producer) => scene.host.datasets.destroy(producer)),
  );
}
