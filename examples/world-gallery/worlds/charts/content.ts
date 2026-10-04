import {
  clientAssetSource,
  type AnimationWorldClient,
  type AnimationTrack,
  type DatasetProducer,
  type DatasetPage,
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
} from "@ipp/react";
import type { GallerySceneContext } from "../../shared/scene.js";
import type { Plot3dContract } from "../charts3d/content.js";
import {
  CHART_CATALOG,
  CHART_SCHEMA,
  CHART_SURFACE,
  type ChartSpec,
} from "./catalog.js";
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
  streamKey,
  streamRows,
  streamWindows,
  type ChartDataMode,
  type ChartWindow,
} from "./streaming.js";
import { declarePlot } from "./shared/declare-plot.js";

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
  readonly controllers: bigint[];
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
  private readonly clips = new Map<string, ClientAssetSource>();
  private nextClip = 710100n;
  private sourceGeneration = 0;
  mode: ChartDataMode = "buffer";
  window: ChartWindow = "count";
  playing = true;
  private readonly attachments: ReactWorldRoot[] = [];
  private readonly highlights = new Map<string, string>();
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
          canvas: { extent: [600, 360], unitsPerMetre: 60 },
          selectedSystems: [
            "ipp.animation",
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
            { width: 600, height: 360 },
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
          unitsPerMetre: 60,
          extent: [600, 360],
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
        controllers: [],
      };
      this.charts.push(chart);
      await this.declare(chart);
      await this.animate(chart);
    }
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
      color_column: spec.component.includes("Pie") ? "color" : "",
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
      chartColumnDefinitions(this.contract, spec),
      [
        series([0, 0.8, 1, 1]),
        ...(spec.secondSeries ? [series([1, 0.6, 0.1, 1], "y2")] : []),
      ],
      [],
      spec.style,
      { y: 1, y2: 1, radius: 1, height: 1, value: 0 },
      chart.sourceKind === "streaming"
        ? { windows: chart.windows, encodeWindows }
        : undefined,
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
      for (const controller of [...chart.controllers]) {
        await chart.client.deleteAnimationController(controller);
        chart.controllers.splice(chart.controllers.indexOf(controller), 1);
      }
      const clip = this.clips.get(chart.spec.id);
      if (clip) {
        await chart.client.releaseAsset(clip);
        this.clips.delete(chart.spec.id);
      }
      const root = this.roots.get(chart.spec.id);
      if (root) {
        await root.render(null);
        await root.unmount();
        this.roots.delete(chart.spec.id);
      }
    }
    this.highlights.clear();
    for (const [name, producer] of this.producers) {
      await this.context.canvas.host.datasets.destroy(producer);
      this.producers.delete(name);
    }
  }

  /** The caller drains ingestion before replacing consumed sources and bindings. */
  async dataSource(mode: ChartDataMode) {
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
            [{ operation: "append", rows: chart.spec.rows }],
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
      await this.animate(chart);
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
      const outcome = await this.context.canvas.host.datasets.update(
        chart.producer,
        [
          {
            operation: "append",
            rows: streamRows(chart.spec, sequence, elapsed),
          },
        ],
      );
      if (outcome.failure)
        throw new Error(`Live ingestion failed: ${outcome.failure.reason}`);
    }
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

  private async animate(chart: GalleryChart) {
    const { client, entity, spec } = chart;
    const binding = client.components[this.bindingComponent(chart)]!;
    const tracks: AnimationTrack[] = ["y", "y2", "height", "radius"].map(
      (column) => ({
        property: { component: binding.id, name: `${column}_parameter` },
        keys: [1, 0.62, 1].map((value, index) => ({
          time: index * 3,
          value: { kind: "dynamic", value: { kind: "f32", value } },
        })),
      }),
    );
    if (spec.component.includes("Pie")) {
      tracks.push({
        property: { component: binding.id, name: "value_parameter" },
        keys: [0, 1, 0].map((value, index) => ({
          time: index * 3,
          value: { kind: "dynamic", value: { kind: "f32", value } },
        })),
      });
    }
    const asset = clientAssetSource(client.session, 10, this.nextClip++);
    this.clips.set(spec.id, asset);
    await client.registerAsset(
      asset,
      client.encodeAnimationClip({ duration: 6, tracks }).buffer,
    );
    const controller = await client.createAnimationController({
      looping: true,
      drivers: tracks.map((track, index) => ({
        source: asset.source,
        track: index,
        target: entity,
        property: track.property!,
      })),
    });
    chart.controllers.push(controller);
    await client.controlAnimationController(controller, { action: "play" });
    if (!this.playing) {
      await client.controlAnimationController(controller, { action: "pause" });
      await client.controlAnimationController(controller, {
        action: "seek",
        time: 0,
      });
    }
  }

  async inspections(): Promise<Map<AnimationWorldClient, Inspection>> {
    const clients = new Set([
      this.client,
      ...this.charts.map((chart) => chart.client),
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

  async ready() {
    const deadline = performance.now() + 30_000;
    for (;;) {
      this.context.signal.throwIfAborted();
      const inspections = await this.inspections();
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
        resources.every((resource) => resource.status === "loaded") &&
        this.charts.every((chart) =>
          chart.controllers.every((id) =>
            inspections
              .get(chart.client)
              ?.controllers?.some(
                (controller) =>
                  controller.id === id &&
                  controller.state === (this.playing ? "playing" : "paused"),
              ),
          ),
        )
      )
        return;
      if (performance.now() > deadline)
        throw new Error("Chart sources and animation did not become ready");
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
  }

  async playback(playing: boolean, time?: number) {
    this.playing = playing;
    for (const chart of this.charts)
      for (const controller of chart.controllers) {
        if (!playing || time !== undefined)
          await chart.client.controlAnimationController(controller, {
            action: "pause",
          });
        if (time !== undefined)
          await chart.client.controlAnimationController(controller, {
            action: "seek",
            time,
          });
        if (playing)
          await chart.client.controlAnimationController(controller, {
            action: "play",
          });
      }
  }

  async highlight(
    hover: ChartHitIdentity | null,
    selection: ChartHitIdentity | null,
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
      if ((this.highlights.get(chart.spec.id) ?? "") === identity) continue;
      const component = chart.client.components[chart.spec.component]!,
        labels = component.fields.labels!;
      successfulBatch(
        await chart.client.batch([
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
                  rows: new Map(values.map((value, index) => [index, value])),
                }),
              },
            },
          },
        ]),
      );
      this.highlights.set(chart.spec.id, identity);
    }
  }

  async changeSamples(changed: boolean) {
    if (this.mode !== "buffer")
      throw new Error("Changed samples apply to fixed datasets only");
    for (const chart of this.charts) {
      const original = [...chart.spec.rows[0]!],
        y = original[1]!;
      if (y.kind === "f32")
        original[1] = { kind: "f32", value: y.value * (changed ? 0.5 : 1) };
      const outcome = await this.context.canvas.host.datasets.update(
        chart.producer,
        [{ operation: "edit", row: 1n, values: original }],
      );
      if (outcome.failure)
        throw new Error(`Chart source edit failed: ${outcome.failure.reason}`);
    }
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
    for (const chart of this.charts)
      for (const controller of chart.controllers)
        await attempt(() => chart.client.deleteAnimationController(controller));
    for (const root of this.roots.values()) {
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
    for (const chart of this.charts) {
      const clip = this.clips.get(chart.spec.id);
      if (clip) await attempt(() => chart.client.releaseAsset(clip));
    }
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
