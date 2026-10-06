import {
  clientAssetSource,
  DynamicProperty,
  type AnimationWorldClient,
  type DatasetProducer,
  type DatasetPage,
  type DatasetValue,
  type DataBindingPage,
  type ClientAssetSource,
  type RowPropertyValue,
  type WorldReference,
  type Inspection,
} from "@ipp/client";
import { createElement as h } from "react";
import {
  AttachedWorld,
  Entity,
  createRoot,
  type ReactWorldRoot,
  type DataWindow,
  PlotLegend,
  plotLegendPlacement,
  plotLegendSize,
  plotColorScaleColor,
  type PlotLegendProps,
} from "@ipp/react";
import type { GallerySceneContext } from "../../shared/scene.js";
import type { Plot3dContract } from "../charts3d/content.js";
import {
  CHART_CATALOG,
  CHART_SCHEMA,
  CHART_SURFACE,
  CHART_CANVAS_EXTENT,
  CHART_UNITS_PER_METRE,
  CHART_LEGEND_WIDTH,
  CHART_LEGEND_GAP,
  type ChartSpec,
} from "./catalog.js";
import { CHART_COLORS, CHART_HEIGHT_SCALE } from "./colors.js";
import { chartColumnDefinitions } from "./columns.js";
import {
  aliasId,
  createEntity,
  componentFields,
  insertComponent,
  successfulBatch,
} from "./shared/commands.js";
import {
  STREAM_SCHEMA,
  streamInterpolationKey,
  streamKey,
  streamRows,
  streamWindows,
  type ChartDataMode,
  type ChartWindow,
} from "./streaming.js";
import { declarePlot } from "./shared/declare-plot.js";

/** Source column feeding each interpolated output in the chart sample layout. */
const SOURCE_COLUMNS = {
  y: 1,
  y2: 3,
  value: 1,
  radius: 5,
  height: 6,
  color: 7,
} as const;

export interface ChartSurface {
  readonly anchor: bigint;
  readonly component: "CylinderSurface";
  readonly width: number;
  readonly height: number;
  readonly curvature: number;
  readonly unitsPerMetre: number;
  readonly extent: readonly [number, number];
}

export interface GalleryChart {
  readonly spec: ChartSpec;
  readonly client: AnimationWorldClient;
  readonly world: WorldReference;
  readonly entity: bigint;
  readonly anchor: bigint | null;
  readonly surface: ChartSurface | null;
  source: string;
  producer: DatasetProducer;
  sourceKind: ChartDataMode;
  windows: readonly DataWindow[];
  readonly font: ClientAssetSource;
  frame: ChartSpec["frame"];
  readonly legend: GalleryChartLegend;
}

export interface GalleryChartLegend {
  readonly client: AnimationWorldClient;
  readonly world: WorldReference;
  readonly anchor: bigint;
  readonly id: string;
  readonly extent: readonly [number, number];
  /** Bounds in the plot's own Canvas coordinates or spatial XY plane. */
  readonly bounds: readonly [number, number, number, number];
  readonly center: readonly [number, number, number];
  readonly surface: "CylinderSurface" | "FlatSurface";
}

interface OwnedChild {
  readonly world: WorldReference;
  client?: AnimationWorldClient;
}

export interface ChartHitIdentity {
  readonly chart: string;
  readonly series: number;
  readonly rowId: bigint;
}

/** Cylindrical Canvas charts own children; spatial charts share the runner's camera World. */
export class ChartContent {
  readonly charts: GalleryChart[] = [];
  private readonly entities: {
    client: AnimationWorldClient;
    entity: bigint;
  }[] = [];
  private readonly roots = new Map<string, ReactWorldRoot>();
  private readonly legendRoots: ReactWorldRoot[] = [];
  private sourceGeneration = 0;
  mode: ChartDataMode = "buffer";
  window: ChartWindow = "count";
  private readonly attachments: ReactWorldRoot[] = [];
  private readonly highlights = new Map<string, string>();
  private readonly adaptiveAxes = new Map<string, boolean>();
  private automaticRange = false;
  private smoothChanges = true;
  private readonly sampleTargets = new Map<
    GalleryChart,
    readonly DatasetValue[]
  >();
  /** Newest live snapshot of each positional chart, row for row with its slots. */
  private readonly snapshotTargets = new Map<
    GalleryChart,
    readonly (readonly DatasetValue[])[]
  >();
  private readonly producers = new Map<string, DatasetProducer>();
  private readonly assets: {
    client: AnimationWorldClient;
    asset: ClientAssetSource;
  }[] = [];
  private readonly children: OwnedChild[] = [];
  readonly client: AnimationWorldClient;
  readonly contract: Plot3dContract;

  constructor(readonly context: GallerySceneContext) {
    this.client = context.canvas.client as AnimationWorldClient;
    this.contract = context.contract as unknown as Plot3dContract;
  }

  private async font(
    client: AnimationWorldClient,
    bytes: Uint8Array<ArrayBuffer>,
  ) {
    const asset = clientAssetSource(client.session, 17, 710001n);
    this.assets.push({ client, asset });
    await client.registerAsset(asset, bytes.buffer);
    return asset;
  }

  private async create(
    client: AnimationWorldClient,
    commands: Parameters<AnimationWorldClient["batch"]>[0],
  ) {
    const outcome = await client.batch(commands);
    for (const alias of outcome.aliases)
      this.entities.push({ client, entity: alias.id });
    return aliasId(outcome, 1);
  }

  async open() {
    const bytes = await this.context.assets.readBytes(
      "/target/font-assets/shure-tech-mono.ippf",
      this.context.signal,
    );
    const rootFont = await this.font(this.client, bytes);
    for (const spec of CHART_CATALOG) {
      this.context.signal.throwIfAborted();
      const canvas = spec.component.endsWith("2d");
      let client = this.client,
        font = rootFont;
      let surface: ChartSurface | null = null;
      if (canvas) {
        const created = await this.context.canvas.host.createWorld({
          symbolicId: `gallery-charts/${this.client.session}/${spec.id}`,
          temporary: true,
          canvas: {
            extent: [...CHART_CANVAS_EXTENT],
            unitsPerMetre: CHART_UNITS_PER_METRE,
          },
          selectedSystems: [
            "ipp.asset-dependencies",
            "ipp.data-bindings",
            "ipp.plot",
            "ipp.canvas",
          ],
        });
        const child: OwnedChild = { world: created.reference };
        this.children.push(child);
        client = (await this.context.canvas.host.openWorld(
          created.reference,
        )) as AnimationWorldClient;
        child.client = client;
        font = await this.font(client, bytes);
        await this.create(client, [
          createEntity(1, `chart-background-${spec.id}`),
          insertComponent(
            client,
            "CanvasStyle",
            { kind: "alias", alias: 1 },
            { red: 0.015, green: 0.025, blue: 0.04, alpha: 1 },
          ),
          insertComponent(
            client,
            "CanvasBox",
            { kind: "alias", alias: 1 },
            { width: CHART_CANVAS_EXTENT[0], height: CHART_CANVAS_EXTENT[1] },
          ),
          createEntity(2, `chart-title-${spec.id}`),
          insertComponent(
            client,
            "CanvasStyle",
            { kind: "alias", alias: 2 },
            {
              x: 600 - 24 - spec.title.length * 9,
              y: 2,
              red: 0,
              green: 0.8,
              blue: 1,
              alpha: 1,
            },
          ),
          insertComponent(
            client,
            "CanvasText",
            { kind: "alias", alias: 2 },
            {
              text: spec.title.toUpperCase(),
              source: font.source,
              font_size: 14,
            },
          ),
        ]);
        const anchor = await this.create(this.client, [
          createEntity(1, `chart-surface-${spec.id}`),
          insertComponent(
            this.client,
            "Transform",
            { kind: "alias", alias: 1 },
            {
              x: spec.position[0],
              y: spec.position[1],
              z: spec.position[2],
              qx: spec.rotation[0],
              qy: spec.rotation[1],
              qz: spec.rotation[2],
              qw: spec.rotation[3],
            },
          ),
          insertComponent(
            this.client,
            "CylinderSurface",
            { kind: "alias", alias: 1 },
            { ...CHART_SURFACE },
          ),
        ]);
        surface = {
          anchor,
          component: "CylinderSurface",
          ...CHART_SURFACE,
          unitsPerMetre: CHART_UNITS_PER_METRE,
          extent: CHART_CANVAS_EXTENT,
        };
        const root = createRoot(this.client, {
          host: this.context.canvas.host,
        });
        this.attachments.push(root);
        await root.render(
          h(
            Entity,
            { bindTo: `chart-surface-${spec.id}` },
            h(AttachedWorld, {
              anchor: `chart-surface-${spec.id}`,
              child: { borrow: created.reference },
              attachment: { mode: "surface-canvas" },
            }),
          ),
        );
      }
      const world = client.worldReference;
      if (!world) throw new Error(`Chart ${spec.id} has no World`);
      const source = `datasets://gallery-charts/${this.client.session}/${spec.id}`;
      const producer = await this.context.canvas.host.datasets.create(
        source,
        "buffer",
        CHART_SCHEMA,
      );
      this.producers.set(source, producer);
      const outcome = await this.context.canvas.host.datasets.update(producer, [
        { operation: "append", rows: spec.rows.map((row) => [...row]) },
      ]);
      if (outcome.failure)
        throw new Error(`Chart ingestion failed: ${outcome.failure.reason}`);
      const entity = await this.create(client, [
        createEntity(1, `chart-${spec.id}`),
        insertComponent(
          client,
          canvas ? "CanvasStyle" : "Transform",
          { kind: "alias", alias: 1 },
          canvas
            ? { x: 0, y: 0 }
            : {
                x: spec.position[0],
                y: spec.position[1],
                z: spec.position[2],
                qx: spec.rotation[0],
                qy: spec.rotation[1],
                qz: spec.rotation[2],
                qw: spec.rotation[3],
              },
        ),
        insertComponent(
          client,
          canvas ? "PlotFrame2d" : "PlotFrame3d",
          { kind: "alias", alias: 1 },
          { ...spec.frame, source: font.source },
        ),
      ]);
      const chart: GalleryChart = {
        spec,
        client,
        world,
        entity,
        anchor: surface?.anchor ?? null,
        surface,
        source,
        producer,
        sourceKind: "buffer",
        windows: [],
        font,
        frame: spec.frame,
        legend: await this.legend(spec, entity, client, font, surface),
      };
      this.charts.push(chart);
      await this.declare(chart);
    }
  }

  private async legend(
    spec: ChartSpec,
    plotEntity: bigint,
    plotClient: AnimationWorldClient,
    plotFont: ClientAssetSource,
    surface: ChartSurface | null,
  ): Promise<GalleryChartLegend> {
    const id = `chart-legend-${spec.id}`;
    const descriptor = {
      id,
      font: plotFont.source,
      title: spec.legendTitle,
      width: CHART_LEGEND_WIDTH,
      ...(spec.legendEntries
        ? { entries: spec.legendEntries }
        : { scale: CHART_HEIGHT_SCALE }),
    } satisfies PlotLegendProps;
    const size = plotLegendSize(descriptor);
    const canvas = surface !== null;
    const depth = canvas ? 0 : Number(spec.frame.depth);
    const density = CHART_UNITS_PER_METRE;
    const placement = plotLegendPlacement({
      bounds: [0, 0, Number(spec.frame.width), Number(spec.frame.height)],
      size: canvas ? size : [size[0] / density, size[1] / density],
      yDirection: canvas ? "down" : "up",
      origin: "bottom-left",
      gap: canvas ? CHART_LEGEND_GAP : CHART_LEGEND_GAP / density,
    });
    let client = plotClient,
      anchor = surface?.anchor ?? 0n;
    if (!canvas) {
      const created = await this.context.canvas.host.createWorld({
        symbolicId: `gallery-charts/${this.client.session}/${spec.id}/legend`,
        temporary: true,
        canvas: { extent: [...size], unitsPerMetre: density },
        selectedSystems: ["ipp.asset-dependencies", "ipp.canvas"],
      });
      const child: OwnedChild = { world: created.reference };
      this.children.push(child);
      client = (await this.context.canvas.host.openWorld(
        created.reference,
      )) as AnimationWorldClient;
      child.client = client;
      anchor = await this.create(this.client, [
        createEntity(1, `chart-legend-surface-${spec.id}`),
        insertComponent(
          this.client,
          "Transform",
          { kind: "alias", alias: 1 },
          {
            x: placement.center[0],
            y: placement.center[1],
            z: depth,
          },
        ),
        insertComponent(
          this.client,
          "FlatSurface",
          { kind: "alias", alias: 1 },
          { width: size[0] / density, height: size[1] / density },
        ),
        {
          kind: "placeEntity",
          entity: { kind: "alias", alias: 1 },
          placement: {
            parent: { kind: "handle", id: plotEntity },
            before: null,
          },
        },
      ]);
      const attachment = createRoot(this.client, {
        host: this.context.canvas.host,
      });
      this.attachments.push(attachment);
      await attachment.render(
        h(
          Entity,
          { bindTo: `chart-legend-surface-${spec.id}` },
          h(AttachedWorld, {
            anchor: `chart-legend-surface-${spec.id}`,
            child: { borrow: created.reference },
            attachment: { mode: "surface-canvas" },
          }),
        ),
      );
    }
    const root = createRoot(client);
    this.legendRoots.push(root);
    await root.render(
      h(PlotLegend, {
        ...descriptor,
        font: plotFont.source,
        x: canvas ? placement.canvasPosition[0] : 0,
        y: canvas ? placement.canvasPosition[1] : 0,
      }),
    );
    const world = client.worldReference;
    if (!world) throw new Error(`Legend ${spec.id} has no World`);
    return {
      client,
      world,
      anchor,
      id,
      extent: canvas ? CHART_CANVAS_EXTENT : size,
      bounds: placement.bounds,
      center: [placement.center[0], placement.center[1], depth],
      surface: canvas ? "CylinderSurface" : "FlatSurface",
    };
  }

  private async declare(chart: GalleryChart) {
    const { client, spec, source } = chart;
    const series = (
      color: readonly number[],
      y = "y",
    ): Readonly<Record<string, RowPropertyValue>> => ({
      name: y === "y" ? "Group A" : "Group B",
      x: "x",
      y,
      z: spec.id === "single-row" ? "" : "z",
      value: spec.component.includes("Pie") ? "value" : y,
      radius: "radius",
      height: "height",
      color_column: spec.colorByRow ? "color" : "",
      color,
      visible: true,
    });
    const encodeWindows = (windows: readonly DataWindow[]) => {
      const contract = this.context.contract as unknown as {
        encodeDataWindows(
          windows: readonly DataWindow[],
        ): Uint8Array<ArrayBuffer>;
      };
      return contract.encodeDataWindows(windows);
    };
    const root = await declarePlot(
      client,
      this.contract,
      `chart-${spec.id}`,
      spec.component,
      source,
      chartColumnDefinitions(this.contract),
      [
        series(CHART_COLORS[0]),
        ...(spec.secondSeries ? [series(CHART_COLORS[1], "y2")] : []),
      ],
      [],
      spec.style,
      {},
      chart.sourceKind === "streaming"
        ? {
            windows: chart.windows,
            encodeWindows,
            interpolationKey: streamInterpolationKey(spec),
          }
        : undefined,
      this.smoothChanges ? this.interpolationRates(chart) : {},
    );
    this.roots.set(spec.id, root);
  }

  bindingComponent(chart: GalleryChart) {
    return chart.sourceKind === "streaming"
      ? "StreamingDataSourceBinding"
      : "BufferDataSourceBinding";
  }

  private async removeDeclarations() {
    for (const chart of this.charts) {
      const root = this.roots.get(chart.spec.id);
      if (root) {
        await root.render(null);
        await root.unmount();
        this.roots.delete(chart.spec.id);
      }
    }
    this.highlights.clear();
    this.adaptiveAxes.clear();
    this.sampleTargets.clear();
    this.snapshotTargets.clear();
    for (const [name, producer] of this.producers) {
      await this.context.canvas.host.datasets.destroy(producer);
      this.producers.delete(name);
    }
  }

  /** The caller drains ingestion before replacing consumed sources and bindings. */
  async dataSource(mode: ChartDataMode, changed = false, expanded = false) {
    if (mode === this.mode) return;
    await this.removeDeclarations();
    this.mode = mode;
    ++this.sourceGeneration;
    for (const chart of this.charts) {
      const key = mode === "streaming" ? streamKey(chart.spec) : chart.spec.id;
      const source = `datasets://gallery-charts/${this.client.session}/${mode}/${this.sourceGeneration}/${key}`;
      let producer = this.producers.get(source);
      if (!producer) {
        producer = await this.context.canvas.host.datasets.create(
          source,
          mode,
          mode === "streaming" ? STREAM_SCHEMA : CHART_SCHEMA,
        );
        this.producers.set(source, producer);
        if (mode === "buffer") {
          const outcome = await this.context.canvas.host.datasets.update(
            producer,
            [
              {
                operation: "append",
                rows: chart.spec.rows.map((row, index) =>
                  index === 0
                    ? this.firstSample(chart.spec, changed, expanded)
                    : row,
                ),
              },
            ],
          );
          if (outcome.failure)
            throw new Error(
              `Chart ingestion failed: ${outcome.failure.reason}`,
            );
        }
      }
      chart.source = source;
      chart.producer = producer;
      chart.sourceKind = mode;
      chart.windows =
        mode === "streaming" ? streamWindows(chart.spec, this.window) : [];
      chart.frame =
        mode === "streaming" && chart.spec.component === "PlotLine2d"
          ? { ...chart.spec.frame, automatic_x: true, x_title: "ELAPSED / S" }
          : chart.spec.frame;
      if (!chart.spec.component.includes("Pie"))
        chart.frame = { ...chart.frame, automatic_y: this.automaticRange };
      successfulBatch(
        await chart.client.batch([
          insertComponent(
            chart.client,
            chart.spec.component.endsWith("2d") ? "PlotFrame2d" : "PlotFrame3d",
            { kind: "handle", id: chart.entity },
            { ...chart.frame, source: chart.font.source },
          ),
        ]),
      );
      await this.declare(chart);
    }
    await this.ready();
  }

  async dataWindow(profile: ChartWindow) {
    this.window = profile;
    if (this.mode !== "streaming") return;
    for (const chart of this.charts) {
      chart.windows = streamWindows(chart.spec, profile);
      const contract = this.context.contract as unknown as {
        encodeDataWindows(
          windows: readonly DataWindow[],
        ): Uint8Array<ArrayBuffer>;
      };
      successfulBatch(
        await chart.client.batch(
          componentFields(chart.client, "StreamingDataSourceBinding", {
            windows: contract.encodeDataWindows(chart.windows),
          }).map((field) => ({
            kind: "setField" as const,
            entity: { kind: "handle" as const, id: chart.entity },
            component: chart.client.components.StreamingDataSourceBinding!.id,
            field,
          })),
        ),
      );
    }
    await this.ready();
  }

  async append(sequence: number, elapsed: number) {
    if (this.mode !== "streaming")
      throw new Error("Live feed has no streaming bindings");
    const sent = new Set<string>();
    for (const chart of this.charts) {
      if (sent.has(chart.source)) continue;
      sent.add(chart.source);
      const rows = streamRows(chart.spec, sequence, elapsed);
      const outcome = await this.context.canvas.host.datasets.update(
        chart.producer,
        [{ operation: "append", rows }],
      );
      if (outcome.failure)
        throw new Error(`Live ingestion failed: ${outcome.failure.reason}`);
      if (streamInterpolationKey(chart.spec) === "position")
        this.snapshotTargets.set(chart, rows);
    }
    // Observe Host evaluation before reconciling picked rows with their current windows.
    await this.client.waitForFrame();
  }

  async sources(): Promise<DatasetPage[]> {
    const pages: DatasetPage[] = [];
    for (const name of this.producers.keys()) {
      let page = await this.context.canvas.host.datasets.read(name, {
        limit: 128,
      });
      const complete = { ...page, rows: [...page.rows] };
      while (page.nextOffset !== null) {
        if (complete.rows.length >= 512)
          throw new Error("Chart source exceeds diagnostic row bound");
        page = await this.context.canvas.host.datasets.read(name, {
          incarnation: complete.incarnation,
          offset: page.nextOffset,
          limit: 128,
        });
        complete.rows.push(...page.rows);
      }
      complete.nextOffset = null;
      pages.push(complete);
    }
    return pages;
  }

  async inspections(): Promise<Map<AnimationWorldClient, Inspection>> {
    const clients = new Set([
      this.client,
      ...this.charts.map((chart) => chart.client),
      ...this.charts.map((chart) => chart.legend.client),
    ]);
    return new Map(
      await Promise.all(
        [...clients].map(async (client) => {
          const manifest = client.manifest;
          if (!manifest) throw new Error("Chart World manifest unavailable");
          const [inspection, canvas] = await Promise.all([
            client.inspect(),
            manifest.systems.includes("ipp.canvas")
              ? client.inspectPage({ collection: "canvas" })
              : Promise.resolve(null),
          ]);
          return [
            client,
            { ...inspection, canvas: canvas?.canvas ?? null },
          ] as const;
        }),
      ),
    );
  }

  private async readiness() {
    const clients = new Set([
      this.client,
      ...this.charts.map((chart) => chart.client),
      ...this.charts.map((chart) => chart.legend.client),
    ]);
    const pages = async (client: AnimationWorldClient) => {
      const result = [];
      let after = 0n;
      do {
        const page = await client.inspectPage({
          collection: "resources",
          after,
        });
        result.push(page);
        after = page.next;
      } while (after !== 0n);
      return result;
    };
    return new Map(
      await Promise.all(
        [...clients].map(async (client) => {
          const resources = await pages(client);
          return [
            client,
            {
              resources: resources.flatMap((page) => page.resources),
            },
          ] as const;
        }),
      ),
    );
  }

  async ready() {
    const deadline = performance.now() + 30_000;
    for (;;) {
      this.context.signal.throwIfAborted();
      const inspections = await this.readiness();
      const resources = [...inspections.values()].flatMap(
        (world) => world.resources,
      );
      const failure = resources.find(
        (resource) => resource.status === "failed",
      );
      if (failure) throw new Error(`Chart resource failed: ${failure.source}`);
      const views = await Promise.all(
        this.charts.map((chart) =>
          this.context.canvas.host.datasets.bindingView(
            chart.client.session,
            chart.entity,
          ),
        ),
      );
      if (
        this.charts.length === CHART_CATALOG.length &&
        views.every(
          (view) => view.availability.reason === "Ready" && !view.dirty,
        ) &&
        resources.every((resource) => resource.status === "loaded")
      )
        return;
      if (performance.now() > deadline)
        throw new Error("Chart sources and assets did not become ready");
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
  }

  async highlight(
    hover: ChartHitIdentity | null,
    selection: ChartHitIdentity | null,
    adaptiveAxes = false,
  ) {
    for (const chart of this.charts) {
      const active = [selection, hover].filter(
        (hit): hit is NonNullable<typeof hit> =>
          hit !== null && hit.chart === chart.spec.id,
      );
      const values = active
        .filter(
          (hit, index) =>
            active.findIndex(
              (other) =>
                other.rowId === hit.rowId && other.series === hit.series,
            ) === index,
        )
        .map((hit) => ({
          series: hit.series,
          row_id: hit.rowId.toString(),
          text: `${hit === selection ? "SELECTED" : "HOVER"} / ROW ${hit.rowId}`,
          highlighted: true,
          connector: true,
          offset: chart.spec.component.endsWith("2d") ? [18, -32] : [1, -1],
        }));
      const identity = values
        .map((value) => `${value.series}:${value.row_id}:${value.text}`)
        .join("|");
      const labelsChanged =
        (this.highlights.get(chart.spec.id) ?? "") !== identity;
      const adaptive = adaptiveAxes && active.length > 0;
      const axesChanged =
        chart.spec.component.endsWith("3d") &&
        (this.adaptiveAxes.get(chart.spec.id) ?? false) !== adaptive;
      if (!labelsChanged && !axesChanged) continue;
      const component = chart.client.components[chart.spec.component]!,
        labels = component.fields.labels!;
      successfulBatch(
        await chart.client.batch([
          ...(axesChanged
            ? componentFields(chart.client, "PlotFrame3d", {
                adaptive_axes: adaptive,
              }).map((field) => ({
                kind: "setField" as const,
                entity: { kind: "handle" as const, id: chart.entity },
                component: chart.client.components.PlotFrame3d!.id,
                field,
              }))
            : []),
          ...(labelsChanged
            ? [
                {
                  kind: "setField",
                  entity: { kind: "handle", id: chart.entity },
                  component: component.id,
                  field: {
                    offset: labels.offset,
                    value: {
                      kind: "rows",
                      value: this.contract.encodeRowsTable(labels.rows!, {
                        nextSlot: values.length,
                        rows: new Map(
                          values.map((value, index) => [index, value]),
                        ),
                      }),
                    },
                  },
                } as const,
              ]
            : []),
        ]),
      );
      this.highlights.set(chart.spec.id, identity);
      this.adaptiveAxes.set(chart.spec.id, adaptive);
    }
  }

  async setAutomaticRange(enabled: boolean) {
    this.automaticRange = enabled;
    for (const chart of this.charts) {
      if (chart.spec.component.includes("Pie")) continue;
      chart.frame = { ...chart.frame, automatic_y: enabled };
      const component = chart.spec.component.endsWith("2d")
        ? "PlotFrame2d"
        : "PlotFrame3d";
      successfulBatch(
        await chart.client.batch(
          componentFields(chart.client, component, {
            automatic_y: enabled,
          }).map((field) => ({
            kind: "setField" as const,
            entity: { kind: "handle" as const, id: chart.entity },
            component: chart.client.components[component]!.id,
            field,
          })),
        ),
      );
    }
    await this.ready();
  }

  private interpolationRates(chart: GalleryChart) {
    const value =
      Number(chart.spec.frame.max_y) - Number(chart.spec.frame.min_y);
    return {
      y: value,
      y2: value,
      value,
      height: Number(chart.spec.frame.height),
      // Live snapshots also vary slice radii and surface colours (0..1 lanes).
      ...(chart.sourceKind === "streaming"
        ? {
            radius: Number(chart.spec.frame.height),
            ...(chart.spec.id === "height-surface" ? { color: 1 } : {}),
          }
        : {}),
    };
  }

  async setSmoothChanges(enabled: boolean) {
    this.smoothChanges = enabled;
    for (const chart of this.charts) {
      const component =
        chart.client.components[this.bindingComponent(chart)]!.id;
      successfulBatch(
        await chart.client.batch(
          Object.entries(this.interpolationRates(chart)).map(
            ([output, rate]) =>
              enabled
                ? DynamicProperty.set(
                    { kind: "handle", id: chart.entity },
                    component,
                    `${output}_interp`,
                    { kind: "f32", value: rate },
                  )
                : DynamicProperty.remove(
                    { kind: "handle", id: chart.entity },
                    component,
                    `${output}_interp`,
                  ),
          ),
        ),
      );
    }
    await this.ready();
  }

  private firstSample(
    spec: ChartSpec,
    changed: boolean,
    expanded: boolean,
  ): DatasetValue[] {
    const original = [...spec.rows[0]!],
      y = original[1]!;
    if (y.kind === "f32")
      original[1] = {
        kind: "f32",
        value:
          expanded && !spec.component.includes("Pie")
            ? Number(spec.frame.max_y) * 1.5
            : y.value * (changed ? 0.5 : 1),
      };
    if (spec.id === "height-surface" && original[1]!.kind === "f32")
      original[7] = {
        kind: "vec4",
        value: [...plotColorScaleColor(CHART_HEIGHT_SCALE, original[1]!.value)],
      };
    return original;
  }

  async changeSamples(changed: boolean, expanded = false) {
    if (this.mode !== "buffer")
      throw new Error("Changed samples apply to fixed datasets only");
    // Dataset delivery is serial on this connection; await admission before the next edit.
    for (const chart of this.charts) {
      const original = this.firstSample(chart.spec, changed, expanded);
      try {
        const outcome = await this.context.canvas.host.datasets.update(
          chart.producer,
          [{ operation: "edit", row: 1n, values: original }],
        );
        if (outcome.failure) throw new Error(outcome.failure.reason);
        this.sampleTargets.set(chart, original);
      } catch (error) {
        throw new Error(
          `Chart ${chart.spec.id} sample edit failed: ${error instanceof Error ? error.message : String(error)}`,
          { cause: error },
        );
      }
    }
    await this.client.waitForFrame();
  }

  /**
   * Seconds until displayed interpolated outputs reach their targets: edited
   * fixed samples, or the newest snapshot under each positional live chart in
   * `live`. Bindings are read only at change boundaries with feedback to follow.
   */
  async remainingFeedback(live: ReadonlySet<string> = new Set()) {
    if (!this.smoothChanges) return { duration: 0, time: 0 };
    const pending =
      this.mode === "buffer"
        ? [...this.sampleTargets].map(([chart, target]) =>
            this.remainingMotion(chart, 1, (row) =>
              row.id === 1n ? target : undefined,
            ),
          )
        : [...this.snapshotTargets]
            .filter(([chart]) => live.has(chart.spec.id))
            .map(([chart, targets]) =>
              this.remainingMotion(
                chart,
                targets.length,
                (_row, index) => targets[index],
              ),
            );
    if (pending.length === 0) return { duration: 0, time: 0 };
    const duration = Math.max(0, ...(await Promise.all(pending)));
    // A later completed Host clock is a conservative upper bound for those reads.
    const frame = await this.client.waitForFrame();
    return { duration, time: frame.time };
  }

  private async remainingMotion(
    chart: GalleryChart,
    limit: number,
    target: (
      row: DataBindingPage["rows"][number],
      index: number,
    ) => readonly DatasetValue[] | undefined,
  ) {
    const [first, inspection] = await Promise.all([
      this.context.canvas.host.datasets.bindingView(
        chart.client.session,
        chart.entity,
        { limit },
      ),
      chart.client.inspectPage({
        collection: "entities",
        target: chart.entity,
        limit: 1,
      }),
    ]);
    const rows = [...first.rows];
    for (let page = first; page.nextOffset !== null && rows.length < limit; ) {
      page = await this.context.canvas.host.datasets.bindingView(
        chart.client.session,
        chart.entity,
        { offset: page.nextOffset, limit: limit - rows.length },
      );
      rows.push(...page.rows);
    }
    const properties = inspection.entities
      .find((entity) => entity.id === chart.entity)
      ?.components.find(
        (component) =>
          component.component ===
          chart.client.components[this.bindingComponent(chart)]!.id,
      )?.properties;
    const lanes = (value: DatasetValue | undefined) =>
      value?.kind === "f32"
        ? [value.value]
        : value?.kind === "vec4"
          ? value.value
          : [];
    let remaining = 0;
    for (const [index, row] of rows.entries()) {
      const end = target(row, index);
      if (!end) continue;
      for (const [output, column] of Object.entries(SOURCE_COLUMNS)) {
        const rate = properties?.[`${output}_interp`];
        if (rate?.kind !== "f32" || rate.value <= 0) continue;
        const value =
          row.values[
            first.columns.findIndex((column) => column.name === output)
          ];
        if (!value?.valid) continue;
        const shown = lanes(value.value),
          goal = lanes(end[column]);
        for (const [lane, displayed] of shown.entries())
          if (goal[lane] !== undefined)
            remaining = Math.max(
              remaining,
              Math.abs(displayed - Math.fround(goal[lane]!)) / rate.value,
            );
      }
    }
    return remaining;
  }

  async close() {
    const failures: unknown[] = [];
    const attempt = async (operation: () => Promise<unknown>) => {
      try {
        await operation();
      } catch (error) {
        failures.push(error);
      }
    };
    // Remove declarations and acknowledge detach before releasing the borrowed children.
    for (const root of this.attachments) {
      await attempt(() => root.render(null));
      await attempt(() => root.unmount());
    }
    for (const root of this.roots.values()) {
      await attempt(() => root.render(null));
      await attempt(() => root.unmount());
    }
    for (const root of this.legendRoots) {
      await attempt(() => root.render(null));
      await attempt(() => root.unmount());
    }
    for (const { client, entity } of this.entities)
      await attempt(async () =>
        successfulBatch(
          await client.batch([
            { kind: "delete", entity: { kind: "handle", id: entity } },
          ]),
        ),
      );
    for (const producer of this.producers.values())
      await attempt(() => this.context.canvas.host.datasets.destroy(producer));
    for (const { client, asset } of this.assets)
      await attempt(() => client.releaseAsset(asset));
    for (const child of this.children) {
      if (child.client) await attempt(() => child.client!.close());
      await attempt(() => this.context.canvas.host.destroyWorld(child.world));
    }
    if (failures.length)
      throw new AggregateError(failures, "Chart scene cleanup is incomplete");
  }
}
