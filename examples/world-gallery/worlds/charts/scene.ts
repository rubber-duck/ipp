import type {
  CameraViewMotion,
  PickingWorldClient,
  DataBindingPage,
  GeometryPickHit,
  WorldReference,
} from "@ipp/client";
import { initializeCamera } from "../../shared/camera.js";
import {
  GALLERY_SYSTEMS,
  type GalleryOptions,
  type GallerySceneDefinition,
} from "../../shared/scene.js";
import {
  ChartFeed,
  type ChartDataMode,
  type ChartWindow,
} from "./streaming.js";
import { CHART_RING } from "./catalog.js";
import { ChartContent } from "./content.js";
import { ChartCamera, chartCameraTarget } from "./camera.js";
import { componentFields, successfulBatch } from "./shared/commands.js";

export interface ChartMark {
  readonly chart: string;
  readonly entity: bigint;
  readonly world: WorldReference;
  readonly path: GeometryPickHit["path"];
  readonly series: number;
  readonly rowId: bigint;
  readonly values: DataBindingPage["rows"][number]["values"];
  readonly columns: readonly string[];
}

export const chartScene: GallerySceneDefinition = {
  id: "charts",
  label: "Charts",
  shortLabel: "Charts",
  description:
    "Turn toward animated flat and spatial charts around you, or focus any exhibit in two seconds.",
  defaultOptions: {
    dataMode: "buffer",
    dataWindow: "count",
    streamPlaying: true,
    streamError: null,
    playing: true,
    changed: false,
    focused: "center",
    navigation: "look",
    hover: null,
    selection: null,
  },
  actions: [
    "focus",
    "resetCamera",
    "navigate",
    "hover",
    "select",
    "clearSelection",
    "playback",
    "changeSamples",
    "dataSource",
    "dataWindow",
    "streamPlayback",
  ],
  world: () => ({
    create: {
      selectedSystems: [...GALLERY_SYSTEMS, "ipp.data-bindings", "ipp.plot"],
    },
  }),
  async mount(context, input) {
    const client = context.canvas.client as PickingWorldClient;
    const cameraEntity = await initializeCamera(client);
    const content = new ChartContent(context);
    const camera = new ChartCamera(content.client, cameraEntity);
    let options: GalleryOptions = {
      ...chartScene.defaultOptions,
      ...input,
      focused: "center",
      navigation: "look",
      hover: null,
      selection: null,
    };
    let navigation: "look" | "orbit" = "look";
    let hover: ChartMark | null = null,
      selection: ChartMark | null = null;
    let closing: Promise<void> | undefined;
    let tail = Promise.resolve();
    const listeners = new Set<() => void>();
    const notify = () => {
      options = { ...options, hover, selection };
      for (const listener of listeners) listener();
    };
    const enqueue = <T>(operation: () => Promise<T>) => {
      if (closing)
        return Promise.reject<T>(new Error("The chart scene is disposed"));
      const result = tail.then(operation);
      tail = result.then(
        () => {},
        () => {},
      );
      return result;
    };
    let feedGeneration = 0;
    let reconciliationPending = false;
    const reconcile = async () => {
      const retained = async (mark: ChartMark | null) => {
        if (!mark) return null;
        const chart = content.charts.find(
          (chart) => chart.spec.id === mark.chart,
        );
        if (!chart) return null;
        let page = await context.canvas.host.datasets.bindingView(
          chart.client.session,
          chart.entity,
        );
        for (;;) {
          if (page.rows.some((row) => row.id === mark.rowId)) return mark;
          if (page.nextOffset === null) return null;
          page = await context.canvas.host.datasets.bindingView(
            chart.client.session,
            chart.entity,
            { offset: page.nextOffset },
          );
        }
      };
      hover = await retained(hover);
      selection = await retained(selection);
      await content.highlight(hover, selection);
    };
    const feed = new ChartFeed(
      async (sequence, elapsed) => {
        await content.append(sequence, elapsed);
        await content.ready();
      },
      () => {
        options = { ...options, streamError: feed.error };
        notify();
        if (closing || reconciliationPending || content.mode !== "streaming")
          return;
        const generation = feedGeneration;
        reconciliationPending = true;
        // Ingestion never waits for the action queue: source switches can safely drain it.
        void enqueue(async () => {
          if (
            closing ||
            generation !== feedGeneration ||
            content.mode !== "streaming"
          )
            return;
          await reconcile();
          notify();
        })
          .catch((error) => {
            if (!closing) {
              options = { ...options, streamError: String(error) };
              notify();
            }
          })
          .finally(() => {
            reconciliationPending = false;
          });
      },
    );
    const resumeFeed = () => {
      if (content.mode === "streaming" && options.streamPlaying) feed.start();
    };
    const world = client.worldReference;
    if (!world) throw new Error("Charts require the runner World");
    const output = await context.canvas.host.bindOutput(
      world,
      cameraEntity,
      "camera",
    );
    const binding = async () => {
      const binding = await context.canvas.host.getRootOutputBinding(world);
      if (!binding) throw new Error("Chart camera has no root presentation");
      return binding;
    };
    const mark = async (
      hit: GeometryPickHit | null,
    ): Promise<ChartMark | null> => {
      if (!hit?.row) return null;
      const chart = content.charts.find(
        (chart) =>
          chart.entity === hit.entity &&
          chart.world.id === hit.world.id &&
          chart.world.incarnation === hit.world.incarnation,
      );
      if (!chart) return null;
      const source = (await content.sources()).find(
        (source) => source.name === chart.source,
      );
      if (!source?.rows.some((row) => row.id === hit.row!.rowId)) return null;
      let page = await context.canvas.host.datasets.bindingView(
        chart.client.session,
        chart.entity,
      );
      let row = page.rows.find((row) => row.id === hit.row!.rowId);
      while (!row && page.nextOffset !== null) {
        page = await context.canvas.host.datasets.bindingView(
          chart.client.session,
          chart.entity,
          { offset: page.nextOffset },
        );
        row = page.rows.find((row) => row.id === hit.row!.rowId);
      }
      if (!row) return null;
      return {
        chart: chart.spec.id,
        entity: hit.entity,
        world: hit.world,
        path: hit.path,
        series: hit.row.series,
        rowId: hit.row.rowId,
        values: row.values,
        columns: page.columns.map((column) => column.name),
      };
    };
    const pick = async (args: unknown) => {
      if (args === null) return null;
      const point = args as { x: number; y: number };
      if (!Number.isFinite(point?.x) || !Number.isFinite(point?.y))
        throw new Error("Chart picking requires normalized x/y coordinates");
      const result = await client.query({
        type: "GeometryPickQuery",
        view: { kind: "bound", binding: await binding() },
        x: point.x,
        y: point.y,
        includeViewPlane: false,
      });
      if (!result.ok) throw new Error(result.error);
      return mark(result.hit);
    };
    const actions: Record<string, (args?: unknown) => Promise<unknown>> = {
      async focus(args) {
        const id = String(args ?? "center");
        await camera.move(id);
        navigation = id === "center" ? "look" : "orbit";
        options = { ...options, focused: id, navigation };
      },
      async resetCamera() {
        await actions.focus!("center");
      },
      async navigate(args) {
        await camera.cancel();
        const motion = args as CameraViewMotion;
        if (navigation === "look" && motion.kind === "rotate")
          await camera.turn(motion.yaw, motion.pitch);
        else await client.navigateCamera({ binding: await binding(), motion });
        options = { ...options, focused: null };
      },
      async hover(args) {
        const next = await pick(args);
        if (
          next?.chart === hover?.chart &&
          next?.series === hover?.series &&
          next?.rowId === hover?.rowId
        )
          return;
        hover = next;
        await content.highlight(hover, selection);
      },
      async select(args) {
        selection = await pick(args);
        await content.highlight(hover, selection);
      },
      async clearSelection() {
        selection = null;
        await content.highlight(hover, selection);
      },
      async playback(args) {
        const playback = args as { playing: boolean; time?: number };
        if (
          typeof playback?.playing !== "boolean" ||
          (playback.time !== undefined &&
            (!Number.isFinite(playback.time) || playback.time < 0))
        )
          throw new Error(
            "Playback requires playing and an optional nonnegative time",
          );
        await content.playback(playback.playing, playback.time);
        options = { ...options, playing: playback.playing };
      },
      async dataSource(args) {
        if (args !== "buffer" && args !== "streaming")
          throw new Error("Unknown chart data source");
        const mode: ChartDataMode = args;
        if (mode === content.mode) return;
        ++feedGeneration;
        await feed.pause(true);
        hover = selection = null;
        await content.highlight(null, null);
        try {
          await content.dataSource(mode);
          options = { ...options, dataMode: mode };
          if (mode === "streaming") await feed.prime();
          else if (options.changed) await content.changeSamples(true);
          await content.ready();
          await reconcile();
          resumeFeed();
        } catch (error) {
          options = {
            ...options,
            dataMode: content.mode,
            streamError: error instanceof Error ? error.message : String(error),
          };
          throw error;
        }
      },
      async dataWindow(args) {
        if (args !== "count" && args !== "time")
          throw new Error("Unknown chart data window");
        const profile: ChartWindow = args;
        ++feedGeneration;
        await feed.pause(content.mode === "buffer");
        await content.dataWindow(profile);
        options = { ...options, dataWindow: profile };
        if (content.mode === "streaming") await reconcile();
        resumeFeed();
      },
      async streamPlayback(args) {
        if (typeof args !== "boolean")
          throw new Error("Stream playback requires a boolean");
        options = { ...options, streamPlaying: args };
        if (args) resumeFeed();
        else {
          await feed.pause(content.mode === "buffer");
          await reconcile();
        }
      },
      async changeSamples(args) {
        const changed = args === undefined ? true : Boolean(args);
        await content.changeSamples(changed);
        options = { ...options, changed };
      },
    };
    try {
      await camera.write(chartCameraTarget("center").pose);
      successfulBatch(
        await client.batch(
          componentFields(client, "Camera", {
            projection: 0,
            fov_y: Math.PI / 3,
            near: 0.1,
            far: 250,
            focus_distance: chartCameraTarget("center").distance,
          }).map((field) => ({
            kind: "setField",
            entity: { kind: "handle", id: cameraEntity },
            component: client.components.Camera!.id,
            field,
          })),
        ),
      );
      await content.open();
      const ready = content.ready().then(async () => {
        if (options.playing === false) await content.playback(false);
        if (options.changed) await content.changeSamples(true);
        if (options.dataWindow === "time") await content.dataWindow("time");
        if (options.dataMode === "streaming")
          await actions.dataSource!("streaming");
      });
      tail = ready;
      void ready.catch(() => {});
      return {
        output,
        ready,
        get options() {
          return options;
        },
        subscribe(listener) {
          listeners.add(listener);
          return () => {
            listeners.delete(listener);
          };
        },
        resize(viewport) {
          return enqueue(async () => {
            const aspect = viewport.width / viewport.height;
            if (aspect === camera.aspect) return;
            camera.aspect = aspect;
            const focused = options.focused;
            if (typeof focused !== "string") return;
            if (camera.focus) await camera.move(focused);
            else {
              const target = chartCameraTarget(focused, aspect);
              await camera.write(target.pose, target.distance);
            }
          });
        },
        update(patch) {
          return enqueue(async () => {
            if (
              patch.playing !== undefined &&
              patch.playing !== options.playing
            )
              await actions.playback!({ playing: Boolean(patch.playing) });
            if (
              patch.changed !== undefined &&
              patch.changed !== options.changed
            )
              await actions.changeSamples!(patch.changed);
            if (
              patch.dataMode !== undefined &&
              patch.dataMode !== options.dataMode
            )
              await actions.dataSource!(patch.dataMode);
            if (
              patch.dataWindow !== undefined &&
              patch.dataWindow !== options.dataWindow
            )
              await actions.dataWindow!(patch.dataWindow);
            if (
              patch.streamPlaying !== undefined &&
              patch.streamPlaying !== options.streamPlaying
            )
              await actions.streamPlayback!(patch.streamPlaying);
            notify();
          });
        },
        action(name, args) {
          const action = actions[name];
          if (!action)
            return Promise.reject(new Error(`Unknown chart action: ${name}`));
          return enqueue(async () => {
            const result = await action(args);
            notify();
            return result ?? options;
          });
        },
        async inspect() {
          await tail;
          const inspections = await content.inspections();
          const inspection = inspections.get(content.client)!;
          const controller = camera.focus
            ? inspection.controllers?.find(
                (item) => item.id === camera.focus!.controller,
              )
            : undefined;
          const pose = camera.pose(inspection);
          return {
            data: {
              mode: content.mode,
              window: content.window,
              feed: {
                playing: feed.playing,
                status: feed.status,
                sequence: feed.sequence,
                inFlight: feed.inFlight,
                error: feed.error ?? options.streamError,
              },
              sources: await content.sources(),
            },
            ring: CHART_RING,
            world: inspection,
            worldReference: world,
            session: client.session,
            selectedSystems: client.manifest?.systems ?? [],
            presentation: await context.canvas.host.getRootOutputBinding(world),
            time: inspection.time,
            tick: inspection.tick,
            camera: {
              entity: cameraEntity,
              transform: pose,
              navigation,
              yaw: Math.atan2(
                2 * (pose.qw * pose.qy + pose.qx * pose.qz),
                1 - 2 * (pose.qy ** 2 + pose.qx ** 2),
              ),
              pitch: Math.asin(
                Math.max(
                  -1,
                  Math.min(1, 2 * (pose.qw * pose.qx - pose.qy * pose.qz)),
                ),
              ),
              fields: inspection.entities
                .find((item) => item.id === cameraEntity)
                ?.components.find(
                  (item) => item.component === client.components.Camera!.id,
                )?.fields,
            },
            focus: camera.focus
              ? {
                  ...camera.focus,
                  time: controller?.time ?? 0,
                  state: controller?.state ?? "stopped",
                }
              : null,
            playing: options.playing,
            controllers: [...inspections].flatMap(([owner, state]) =>
              (state.controllers ?? []).map((controller) => ({
                ...controller,
                world: owner.worldReference!,
                session: owner.session,
                chart:
                  content.charts.find(
                    (chart) =>
                      chart.client === owner &&
                      chart.controllers.includes(controller.id),
                  )?.spec.id ?? null,
              })),
            ),
            hover,
            selection,
            charts: await Promise.all(
              content.charts.map(async (chart) => ({
                id: chart.spec.id,
                title: chart.spec.title,
                entity: chart.entity,
                component: chart.spec.component,
                world: chart.world,
                session: chart.client.session,
                selectedSystems: chart.client.manifest?.systems ?? [],
                inspection: inspections.get(chart.client)!,
                anchor: chart.anchor,
                surface: chart.surface,
                controllers: chart.controllers,
                source: chart.source,
                sourceKind: chart.sourceKind,
                bindingComponent: content.bindingComponent(chart),
                windows: chart.windows,
                position: chart.spec.position,
                center: chart.spec.center,
                localCenter: chart.spec.localCenter,
                yaw: chart.spec.yaw,
                rotation: chart.spec.rotation,
                frame: chart.frame,
                binding: await context.canvas.host.datasets.bindingView(
                  chart.client.session,
                  chart.entity,
                ),
              })),
            ),
          };
        },
        dispose() {
          if (!closing) {
            listeners.clear();
            closing = tail
              .catch(() => {})
              .then(async () => {
                ++feedGeneration;
                await feed.pause(true);
                const results = await Promise.allSettled([
                  camera.close(),
                  content.close(),
                ]);
                const failures = results
                  .filter(
                    (result): result is PromiseRejectedResult =>
                      result.status === "rejected",
                  )
                  .map((result) => result.reason);
                if (failures.length)
                  throw new AggregateError(failures, "Charts remain owned");
              });
          }
          return closing;
        },
      };
    } catch (error) {
      ++feedGeneration;
      await feed.pause(true);
      await camera.close().catch(() => {});
      await content.close().catch(() => {});
      throw error;
    }
  },
};
