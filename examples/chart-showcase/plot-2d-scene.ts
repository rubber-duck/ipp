/** Real-source 2D Plot fixture shared by native development and maintained scenarios. */
import type {
  AnimationWorldClient,
  Client,
  DatasetProducer,
  DatasetValue,
  HostClientBase,
  FieldWrite,
  PickingWorldClient,
  RootBinding,
  RowPropertyValue,
} from "@ipp/client";
import { clientAssetSource } from "../../packages/ipp-client/src/asset-sources.js";
import type * as Generated from "@ipp/host-contract";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "./commands.js";

export type Plot2dContract = PlotContract &
  Pick<
    typeof Generated,
    "ExpressionBuilder" | "encodeRowsTable" | "GUI_SKIN_TOKENS"
  >;
export const PLOT_2D_EXTENT = {
  width: 1400,
  height: 1000,
  devicePixelRatio: 1,
} as const;
const DEFAULT_DATA_COLOR = [0.0, 0.8, 1.0, 1.0] as const;
const schema = [
  { name: "x", kind: "f32" },
  { name: "y", kind: "f32" },
  { name: "y2", kind: "f32" },
  { name: "valid", kind: "f32" },
  { name: "color", kind: "vec4" },
] as const;
import { declarePlot } from "./declare-plot.js";
import type { ReactWorldRoot, PlotContract } from "@ipp/react";

type Row = Readonly<Record<string, RowPropertyValue>>;

function data(
  x: number,
  y: number,
  y2 = y,
  valid = 1,
  color: readonly number[] = DEFAULT_DATA_COLOR,
): DatasetValue[] {
  return [
    { kind: "f32", value: x },
    { kind: "f32", value: y },
    { kind: "f32", value: y2 },
    { kind: "f32", value: valid },
    { kind: "vec4", value: [...color] },
  ];
}

function series(color: readonly number[], y = "y", colorColumn = ""): Row {
  return {
    name: "samples",
    x: "x",
    y,
    z: "",
    value: "value",
    radius: "",
    height: "",
    color_column: colorColumn,
    color,
    visible: true,
  };
}

function label(
  row: bigint,
  text: string,
  offset: readonly number[],
  highlighted = false,
): Row {
  return {
    series: 0,
    row_id: row.toString(),
    text,
    offset,
    highlighted,
    connector: true,
  };
}

export interface Plot2dScene {
  readonly host: HostClientBase<Client>;
  readonly contract: Plot2dContract;
  readonly world: Awaited<ReturnType<HostClientBase<Client>["createWorld"]>>;
  readonly client: AnimationWorldClient;
  readonly producers: DatasetProducer[];
  readonly roots: ReactWorldRoot[];
  parameterController?: bigint;
  readonly charts: {
    entity: bigint;
    component: string;
    source: string;
    labelsNextSlot: number;
  }[];
}

function rows(
  scene: Plot2dScene,
  component: string,
  field: string,
  values: readonly Row[],
  nextSlot = values.length,
) {
  const layout = scene.client.components[component]?.fields[field]?.rows;
  if (!layout)
    throw new Error(`Missing generated ${component}.${field} layout`);
  return scene.contract.encodeRowsTable(layout, {
    nextSlot,
    rows: new Map(values.map((value, index) => [index, value])),
  });
}

function rowField(
  scene: Plot2dScene,
  component: string,
  field: string,
  values: readonly Row[],
  nextSlot = values.length,
): FieldWrite {
  const descriptor = scene.client.components[component]?.fields[field];
  if (!descriptor?.rows)
    throw new Error(`Missing generated ${component}.${field} layout`);
  return {
    offset: descriptor.offset,
    value: {
      kind: "rows",
      value: rows(scene, component, field, values, nextSlot),
    },
  };
}

async function setLabels(
  scene: Plot2dScene,
  index: number,
  values: readonly Row[],
) {
  const chart = scene.charts[index]!;
  successfulBatch(
    await scene.client.batch([
      {
        kind: "setField",
        entity: { kind: "handle", id: chart.entity },
        component: scene.client.components[chart.component]!.id,
        field: rowField(
          scene,
          chart.component,
          "labels",
          values,
          chart.labelsNextSlot,
        ),
      },
    ]),
  );
}

/** No client-generated chart geometry: declarations contain only samples, selectors and labels. */
export async function openPlot2d(
  host: HostClientBase<Client>,
  contract: Plot2dContract,
  font: Uint8Array<ArrayBuffer>,
  name: string,
): Promise<Plot2dScene> {
  const {
    accent: CYAN,
    amber: AMBER,
    error: MAGENTA,
    neutral: NEUTRAL,
    page: PAGE,
    surface: SURFACE,
    line: LINE,
  } = contract.GUI_SKIN_TOKENS;
  const world = await host.createWorld({
    symbolicId: `plots-2d/${name}`,
    temporary: true,
    selectedSystems: [
      "ipp.animation",
      "ipp.asset-dependencies",
      "ipp.data-bindings",
      "ipp.plot",
      "ipp.canvas",
    ],
  });
  const client = (await host.openWorld(
    world.reference,
  )) as AnimationWorldClient;
  const scene: Plot2dScene = {
    host,
    contract,
    world,
    client,
    producers: [],
    roots: [],
    charts: [],
  };
  try {
    const fontAsset = clientAssetSource(client.session, 17, 1n);
    await client.registerAsset(fontAsset, font.buffer);
    const definitions: Record<string, Uint8Array<ArrayBuffer>> = {};
    for (const [output, column, divide] of [
      ["x", "x", false],
      ["y", "y", true],
      ["y2", "y2", true],
      ["value", "y", false],
      ["color", "color", false],
    ] as const) {
      const builder = new contract.ExpressionBuilder();
      const input = builder.input(
        `column:${column}`,
        column === "color" ? "vec4" : "f32",
      );
      let expression = divide
        ? builder.binary("divide", input, builder.input("column:valid", "f32"))
        : input;
      if (divide) {
        expression = builder.binary(
          "multiply",
          expression,
          builder.fallback(
            builder.input("parameter", "f32"),
            builder.constant({ kind: "f32", value: 1 }),
          ),
        );
      }
      definitions[output] = builder.encode(expression);
    }

    let alias = 1;
    const decoration = async (
      symbol: string,
      x: number,
      y: number,
      width: number,
      height: number,
      color: readonly number[],
      text?: string,
      fontSize = 18,
    ) => {
      const ref = { kind: "alias" as const, alias: alias++ };
      successfulBatch(
        await client.batch([
          createEntity(ref.alias, symbol),
          insertComponent(client, "CanvasStyle", ref, {
            x,
            y,
            red: color[0]!,
            green: color[1]!,
            blue: color[2]!,
            alpha: color[3]!,
          }),
          insertComponent(
            client,
            text === undefined ? "CanvasBox" : "CanvasText",
            ref,
            text === undefined
              ? { width, height }
              : { text, source: fontAsset.source, font_size: fontSize },
          ),
        ]),
      );
    };
    await decoration("page", 0, 0, 1400, 1000, PAGE);
    await decoration("header", 36, 28, 0, 0, CYAN, "IPP / 2D CHARTS", 30);
    await decoration(
      "subtitle",
      36,
      70,
      0,
      0,
      NEUTRAL,
      "Linear axes  /  authored labels  /  sample highlighting  /  missing-data gaps",
      17,
    );
    await decoration("rule", 36, 105, 1328, 1, CYAN);
    for (const [index, title] of [
      "01 / STRAIGHT LINE",
      "02 / SMOOTH LINE",
      "03 / BAR CHART / SUPPLIED BINS",
      "04 / PIE / RESOURCE ALLOCATION",
    ].entries()) {
      const x = 36 + (index % 2) * 678;
      const y = 132 + Math.floor(index / 2) * 415;
      await decoration(`panel-${index}`, x, y, 650, 385, SURFACE);
      for (const [edge, [dx, dy, w, h]] of [
        [0, 0, 650, 1],
        [0, 384, 650, 1],
        [0, 0, 1, 385],
        [649, 0, 1, 385],
      ].entries()) {
        await decoration(
          `panel-${index}-${edge}`,
          x + dx!,
          y + dy!,
          w!,
          h!,
          LINE,
        );
      }
      await decoration(`title-${index}`, x + 18, y + 16, 0, 0, CYAN, title, 20);
    }

    const addChart = async (
      component: string,
      sourceKey: string,
      position: readonly number[],
      size: readonly number[],
      sourceRows: DatasetValue[][],
      seriesRows: readonly Row[],
      labelRows: readonly Row[],
      extra: Record<string, number> = {},
    ) => {
      const source = `datasets://plots-2d/${name}/${sourceKey}`;
      const producer = await host.datasets.create(source, "buffer", schema);
      scene.producers.push(producer);
      const outcome = await host.datasets.update(producer, [
        { operation: "append", rows: sourceRows },
      ]);
      if (outcome.failure)
        throw new Error(
          `Plot fixture ingestion failed: ${outcome.failure.reason}`,
        );
      const ref = { kind: "alias" as const, alias: alias++ };
      const batch = [
        createEntity(ref.alias, sourceKey),
        insertComponent(client, "CanvasStyle", ref, {
          x: position[0]!,
          y: position[1]!,
        }),
        insertComponent(client, "PlotFrame2d", ref, {
          width: size[0]!,
          height: size[1]!,
          padding_left: 48,
          padding_top: 24,
          padding_right: 24,
          padding_bottom: 48,
          min_x: 0,
          max_x: component === "PlotBars2d" ? 5 : 10,
          min_y: 0,
          max_y: component === "PlotBars2d" ? 100 : 100,
          automatic_x: false,
          automatic_y: false,
          ticks: 5,
          source: fontAsset.source,
          font_size: 14,
          x_title: component === "PlotBars2d" ? "NODE / BIN" : "TIME / S",
          y_title: component === "PlotBars2d" ? "OUTPUT / UNITS" : "SIGNAL / %",
        }),
      ];
      const entity = aliasId(await client.batch(batch), ref.alias);
      scene.roots.push(
        await declarePlot(
          client,
          contract,
          sourceKey,
          component as Parameters<typeof declarePlot>[3],
          source,
          definitions,
          seriesRows,
          labelRows,
          extra,
        ),
      );
      scene.charts.push({
        entity,
        component,
        source,
        labelsNextSlot: labelRows.length,
      });
    };
    const lineRows = [
      data(0, 10, 20),
      data(2, 45, 35),
      data(4, 65, 60),
      data(6, 0, 0, 0),
      data(8, 35, 45),
      data(10, 85, 75),
    ];
    await addChart(
      "PlotLine2d",
      "straight",
      [54, 178],
      [610, 323],
      lineRows,
      [series(CYAN)],
      [label(3n, "sample / 65", [26, -48], true)],
      { interpolation: 0, line_width: 2.5, marker_size: 7 },
    );
    await addChart(
      "PlotLine2d",
      "smooth",
      [732, 178],
      [610, 323],
      lineRows,
      [series(CYAN), series(AMBER, "y2")],
      [label(3n, "sample / 65", [26, -48], true)],
      { interpolation: 1, line_width: 2.5, marker_size: 6 },
    );
    await addChart(
      "PlotBars2d",
      "bars",
      [54, 595],
      [350, 320],
      [data(1, 35), data(2, 65), data(3, 50), data(4, 80)],
      [series(CYAN)],
      [label(2n, "B / 65", [24, -48], true)],
      { gap: 0.28 },
    );
    await addChart(
      "PlotPie2d",
      "pie",
      [732, 595],
      [610, 320],
      [
        data(0, 40, 40, 1, CYAN),
        data(0, 30, 30, 1, NEUTRAL),
        data(0, 20, 20, 1, AMBER),
        data(0, 10, 10, 1, MAGENTA),
      ],
      [series(CYAN, "y", "color")],
      [
        label(1n, "40 / SHARE", [42, -64], true),
        label(2n, "30 / SHARE", [-180, 16]),
        label(3n, "20 / SHARE", [-170, -42]),
        label(4n, "10 / SHARE", [12, -52]),
      ],
    );
    await decoration(
      "bins-heading",
      417,
      615,
      0,
      0,
      AMBER,
      "PRE-BINNED INPUT",
      15,
    );
    await addChart(
      "PlotBars2d",
      "bins",
      [407, 648],
      [255, 230],
      [data(1, 20), data(2, 45), data(3, 65), data(4, 35)],
      [series(AMBER)],
      [],
      { gap: 0.25 },
    );
    const animation = clientAssetSource(client.session, 10, 100n);
    const component = client.components.BufferDataSourceBinding!.id;
    await client.registerAsset(
      animation,
      client.encodeAnimationClip({
        duration: 1,
        tracks: [
          {
            property: { component, name: "y2_parameter" },
            keys: [
              {
                time: 0,
                value: { kind: "dynamic", value: { kind: "f32", value: 1 } },
                interpolation: { kind: "linear" },
              },
              {
                time: 1,
                value: { kind: "dynamic", value: { kind: "f32", value: 0.6 } },
              },
            ],
          },
        ],
      }).buffer,
    );
    scene.parameterController = await client.createAnimationController({
      speed: 0,
      drivers: [
        {
          source: animation.source,
          track: 0,
          target: scene.charts[1]!.entity,
          property: { component, name: "y2_parameter" },
        },
      ],
    });
    await pinPlot2dParameter(scene, 0);
    return scene;
  } catch (error) {
    await closePlot2d(scene);
    throw error;
  }
}

/** Wait for the actual completed binding cut; no client time advancement. */
export async function readyPlot2d(scene: Plot2dScene): Promise<void> {
  for (const chart of scene.charts) {
    const deadline = performance.now() + 30_000;
    for (;;) {
      const page = await scene.host.datasets.bindingView(
        scene.client.session,
        chart.entity,
      );
      if (page.availability.reason === "Ready" && !page.dirty) break;
      if (performance.now() > deadline)
        throw new Error(
          `Plot preparation timed out: ${chart.source} / ${JSON.stringify(page.availability)} / dirty=${page.dirty}`,
        );
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
  }
}

/** Keep source identities while changing data and authored highlight selection. */
export async function changePlot2d(scene: Plot2dScene): Promise<void> {
  for (const index of [0, 1]) {
    const outcome = await scene.host.datasets.update(scene.producers[index]!, [
      { operation: "edit", row: 3n, values: data(4, 30, 50) },
    ]);
    if (outcome.failure) throw new Error("Line source edit failed");
    await setLabels(scene, index, [
      label(5n, "selected / 35", [-130, -54], true),
    ]);
  }
  await pinPlot2dParameter(scene, 1);
  await scene.host.datasets.update(scene.producers[2]!, [
    { operation: "edit", row: 2n, values: data(2, 40) },
  ]);
  await setLabels(scene, 2, [label(4n, "D / 80", [-90, -45], true)]);
  await setLabels(scene, 3, [
    label(2n, "30 / SELECTED", [-180, 20], true),
    label(1n, "40 / SHARE", [42, -64]),
  ]);
}

/** Playback intent only: the Host owns time; captures pin the existing controller. */
export async function pinPlot2dParameter(scene: Plot2dScene, time: number) {
  const controller = scene.parameterController!;
  await scene.client.controlAnimationController(controller, { action: "play" });
  const deadline = performance.now() + 30_000;
  while (
    !(await scene.client.inspect()).controllers?.some(
      (item) => item.id === controller && item.state === "playing",
    )
  ) {
    if (performance.now() > deadline)
      throw new Error("Plot parameter animation did not bind");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  await scene.client.controlAnimationController(controller, {
    action: "pause",
  });
  await scene.client.controlAnimationController(controller, {
    action: "seek",
    time,
  });
}

export async function closePlot2d(scene: Plot2dScene): Promise<void> {
  await Promise.all(scene.roots.map((root) => root.unmount()));
  await scene.client.close();
  await scene.host.destroyWorld(scene.world.reference);
  await Promise.allSettled(
    scene.producers.map((producer) => scene.host.datasets.destroy(producer)),
  );
}

/** Production picks use the exact root binding and normalized Canvas coordinates. */
export async function pickPlot2d(
  scene: Plot2dScene,
  binding: RootBinding,
  changed: boolean,
) {
  const client = scene.client as unknown as PickingWorldClient;
  const pick = (x: number, y: number) =>
    client.query({
      type: "GeometryPickQuery",
      view: { kind: "bound", binding },
      x: x / PLOT_2D_EXTENT.width,
      y: y / PLOT_2D_EXTENT.height,
      includeViewPlane: false,
    });
  const [line, bar, pie] = await Promise.all([
    changed ? pick(532, 365) : pick(317, 290),
    changed ? pick(324, 760) : pick(213, 800),
    changed ? pick(1010, 810) : pick(1120, 743),
  ]);
  return { line, bar, pie };
}

/** Narrow the shared hit union to a data mark for independent fixture assertions. */
export interface PlotMarkPick {
  readonly entity: bigint;
  readonly component: number;
  readonly row: { readonly series: number; readonly rowId: bigint };
}
