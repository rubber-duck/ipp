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
import { CHART_RING, CHART_UNITS_PER_METRE } from "./catalog.js";
import { CHART_HEIGHT_SCALE } from "./colors.js";
import { ChartContent } from "./content.js";
import { ChartCamera, chartCameraTarget, chartFocusBounds } from "./camera.js";
import { ChartActionLane } from "./action-lane.js";
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

type CameraAction =
  | { kind: "focus"; id: string; generation: number }
  | { kind: "navigate"; motion: CameraViewMotion; generation: number }
  | { kind: "interrupt" | "resize"; generation: number };

function sameValue(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (Array.isArray(a) && Array.isArray(b))
    return (
      a.length === b.length &&
      a.every((value, index) => sameValue(value, b[index]))
    );
  if (a && b && typeof a === "object" && typeof b === "object") {
    const first = a as Record<string, unknown>,
      second = b as Record<string, unknown>;
    return (
      Object.keys(first).length === Object.keys(second).length &&
      Object.entries(first).every(
        ([name, value]) =>
          Object.hasOwn(second, name) && sameValue(value, second[name]),
      )
    );
  }
  return false;
}

function sameMark(a: ChartMark | null, b: ChartMark | null) {
  return (
    a === b ||
    (a !== null &&
      b !== null &&
      a.chart === b.chart &&
      a.entity === b.entity &&
      a.world.id === b.world.id &&
      a.world.incarnation === b.world.incarnation &&
      a.series === b.series &&
      a.rowId === b.rowId &&
      sameValue(a.columns, b.columns) &&
      sameValue(a.values, b.values))
  );
}

function combineCameraAction(previous: CameraAction, next: CameraAction) {
  if (
    previous.kind !== "navigate" ||
    next.kind !== "navigate" ||
    previous.generation !== next.generation
  )
    return next;
  const a = previous.motion,
    b = next.motion;
  if (a.kind === "rotate" && b.kind === "rotate")
    return {
      ...next,
      motion: {
        kind: "rotate" as const,
        yaw: a.yaw + b.yaw,
        pitch: a.pitch + b.pitch,
      },
    };
  if (a.kind === "pan" && b.kind === "pan")
    return {
      ...next,
      motion: { kind: "pan" as const, x: a.x + b.x, y: a.y + b.y },
    };
  if (a.kind === "zoom" && b.kind === "zoom")
    return {
      ...next,
      motion: { kind: "zoom" as const, amount: a.amount + b.amount },
    };
  return next;
}

export const chartScene: GallerySceneDefinition = {
  id: "charts",
  label: "Charts",
  shortLabel: "Charts",
  description:
    "Explore Canvas charts curved inward along their ring beside spatial plots, or focus any exhibit in two seconds.",
  defaultOptions: {
    dataMode: "buffer",
    dataWindow: "count",
    streamPlaying: true,
    streamError: null,
    changed: false,
    focused: "center",
    navigation: "look",
    hover: null,
    selection: null,
    cameraInputGeneration: 0,
  },
  actions: [
    "focus",
    "resetCamera",
    "navigate",
    "beginNavigation",
    "endNavigation",
    "hover",
    "select",
    "clearSelection",
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
      cameraInputGeneration: 0,
    };
    let navigation: "look" | "orbit" = "look";
    let hover: ChartMark | null = null,
      selection: ChartMark | null = null;
    let closing: Promise<void> | undefined;
    let tail = Promise.resolve();
    let ready = Promise.resolve();
    let cameraGeneration = 0,
      hoverGeneration = 0;
    let cameraIntent: "focus" | "navigate" = "focus";
    let viewTransition: Promise<void> | undefined;
    let finishViewTransition: (() => void) | undefined;
    const listeners = new Set<() => void>();
    let publishedOptions = options;
    const notify = () => {
      options = { ...options, hover, selection };
      if (
        Object.keys(options).length === Object.keys(publishedOptions).length &&
        Object.entries(options).every(([name, value]) =>
          Object.is(value, publishedOptions[name]),
        )
      )
        return;
      publishedOptions = options;
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
    const highlights = new ChartActionLane<{
      hover: ChartMark | null;
      selection: ChartMark | null;
    }>(async (marks) => content.highlight(marks.hover, marks.selection));
    const highlight = () => highlights.submit({ hover, selection });
    const cameraActions = new ChartActionLane<CameraAction>(async (request) => {
      await ready;
      const current = () => !closing && request.generation === cameraGeneration;
      if (!current()) return;
      if (request.kind === "focus") {
        await camera.move(request.id, current);
      } else if (request.kind === "navigate") {
        await camera.cancel();
        if (!current()) return;
        if (navigation === "look" && request.motion.kind === "rotate")
          await camera.turn(request.motion.yaw, request.motion.pitch);
        else {
          camera.invalidate();
          const currentBinding = await binding();
          if (!current()) return;
          await client.navigateCamera({
            binding: currentBinding,
            motion: request.motion,
          });
        }
      } else if (request.kind === "interrupt") {
        await camera.cancel();
      } else {
        const focused = options.focused;
        if (typeof focused !== "string") return;
        if (camera.focus) await camera.move(focused, current);
        else {
          const target = chartCameraTarget(focused, camera.aspect);
          await camera.write(target.pose, target.distance);
        }
      }
      if (current()) notify();
    }, combineCameraAction);
    const hoverActions = new ChartActionLane<{
      args: unknown;
      generation: number;
    }>(async (request) => {
      await ready;
      if (viewTransition) await viewTransition;
      if (closing || request.generation !== hoverGeneration) return;
      let next: ChartMark | null;
      try {
        next = await pick(request.args);
      } catch (error) {
        if (closing || request.generation !== hoverGeneration) return;
        throw error;
      }
      if (closing || request.generation !== hoverGeneration) return;
      if (sameMark(next, hover)) return;
      hover = next;
      await highlight();
      if (!closing && request.generation === hoverGeneration) notify();
    });
    const clearHover = () => {
      ++hoverGeneration;
      hoverActions.drop();
      hover = null;
      return highlight();
    };
    const interruptCamera = () => {
      cameraIntent = "navigate";
      ++cameraGeneration;
      cameraActions.drop();
      options = {
        ...options,
        focused: null,
        cameraInputGeneration: cameraGeneration,
      };
    };
    const changeView = async <T>(change: () => Promise<T>) => {
      ++hoverGeneration;
      hoverActions.drop();
      viewTransition = new Promise<void>((resolve) => {
        finishViewTransition = resolve;
      });
      try {
        return await change();
      } finally {
        finishViewTransition!();
        finishViewTransition = undefined;
        viewTransition = undefined;
      }
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
          const row = page.rows.find((row) => row.id === mark.rowId);
          if (row) {
            const current = {
              ...mark,
              values: row.values,
              columns: page.columns.map((column) => column.name),
            };
            return sameMark(current, mark) ? mark : current;
          }
          if (page.nextOffset === null) return null;
          page = await context.canvas.host.datasets.bindingView(
            chart.client.session,
            chart.entity,
            { offset: page.nextOffset },
          );
        }
      };
      const previousHover = hover,
        previousSelection = selection;
      const [retainedHover, retainedSelection] = await Promise.all([
        retained(previousHover),
        retained(previousSelection),
      ]);
      if (hover === previousHover) hover = retainedHover;
      if (selection === previousSelection) selection = retainedSelection;
      await highlight();
    };
    const feed = new ChartFeed(
      async (sequence, elapsed) => {
        await content.append(sequence, elapsed);
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
        chartCameraTarget(id, camera.aspect);
        cameraIntent = "focus";
        const generation = ++cameraGeneration;
        cameraActions.drop();
        navigation = id === "center" ? "look" : "orbit";
        options = {
          ...options,
          focused: id,
          navigation,
          cameraInputGeneration: generation,
        };
        const pending = [
          clearHover(),
          cameraActions.submit({ kind: "focus", id, generation }),
        ];
        notify();
        await Promise.all(pending);
      },
      async resetCamera() {
        await actions.focus!("center");
      },
      async navigate(args) {
        const motion = args as CameraViewMotion;
        const values =
          motion?.kind === "rotate"
            ? [motion.yaw, motion.pitch]
            : motion?.kind === "pan"
              ? [motion.x, motion.y]
              : motion?.kind === "zoom"
                ? [motion.amount]
                : [];
        if (
          !values.length ||
          !values.every(
            (value) => typeof value === "number" && Number.isFinite(value),
          )
        )
          throw new Error("Chart navigation requires finite camera motion");
        if (cameraIntent !== "navigate") interruptCamera();
        options = { ...options, focused: null };
        await cameraActions.submit({
          kind: "navigate",
          motion: { ...motion },
          generation: cameraGeneration,
        });
      },
      async beginNavigation() {
        interruptCamera();
        const pending = [
          clearHover(),
          cameraActions.submit({
            kind: "interrupt",
            generation: cameraGeneration,
          }),
        ];
        notify();
        await Promise.all(pending);
      },
      async endNavigation() {
        await cameraActions.settled();
      },
      async hover(args) {
        await hoverActions.submit({ args, generation: ++hoverGeneration });
      },
      async select(args) {
        selection = await pick(args);
        await highlight();
      },
      async clearSelection() {
        selection = null;
        await highlight();
      },
      async dataSource(args) {
        if (args !== "buffer" && args !== "streaming")
          throw new Error("Unknown chart data source");
        const mode: ChartDataMode = args;
        if (mode === content.mode) return;
        await changeView(async () => {
          ++feedGeneration;
          await feed.pause(true);
          hover = selection = null;
          await highlight();
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
              streamError:
                error instanceof Error ? error.message : String(error),
            };
            throw error;
          }
        });
      },
      async dataWindow(args) {
        if (args !== "count" && args !== "time")
          throw new Error("Unknown chart data window");
        const profile: ChartWindow = args;
        await changeView(async () => {
          ++feedGeneration;
          await feed.pause(content.mode === "buffer");
          await content.dataWindow(profile);
          options = { ...options, dataWindow: profile };
          if (content.mode === "streaming") await reconcile();
          resumeFeed();
        });
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
        await changeView(async () => {
          await content.changeSamples(changed);
          await content.client.waitForFrame();
          options = { ...options, changed };
          await reconcile();
        });
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
      ready = content.ready().then(async () => {
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
          if (closing)
            return Promise.reject(new Error("The chart scene is disposed"));
          const aspect = viewport.width / viewport.height;
          if (aspect === camera.aspect) return Promise.resolve();
          camera.aspect = aspect;
          if (typeof options.focused !== "string") return Promise.resolve();
          return cameraActions.submit({
            kind: "resize",
            generation: cameraGeneration,
          });
        },
        update(patch) {
          return enqueue(async () => {
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
          if (closing)
            return Promise.reject(new Error("The chart scene is disposed"));
          const run = async () => {
            const result = await action(args);
            notify();
            return result ?? options;
          };
          return [
            "focus",
            "resetCamera",
            "navigate",
            "beginNavigation",
            "endNavigation",
            "hover",
          ].includes(name)
            ? run()
            : enqueue(run);
        },
        async inspect() {
          await Promise.all([
            tail,
            cameraActions.settled(),
            hoverActions.settled(),
          ]);
          await highlights.settled();
          const inspections = await content.inspections();
          const inspection = inspections.get(content.client)!;
          const controller = camera.focus
            ? inspection.controllers?.find(
                (item) => item.id === camera.focus!.controller,
              )
            : undefined;
          const pose = camera.pose(inspection);
          const bindingInspection = async (
            chart: (typeof content.charts)[number],
          ) => {
            let page = await context.canvas.host.datasets.bindingView(
              chart.client.session,
              chart.entity,
              { limit: 128 },
            );
            const complete = { ...page, rows: [...page.rows] };
            while (page.nextOffset !== null) {
              page = await context.canvas.host.datasets.bindingView(
                chart.client.session,
                chart.entity,
                { offset: page.nextOffset, limit: 128 },
              );
              if (
                page.sourceIncarnation !== complete.sourceIncarnation ||
                page.bindingIncarnation !== complete.bindingIncarnation ||
                page.evaluatedTick !== complete.evaluatedTick ||
                page.totalRows !== complete.totalRows
              )
                throw new Error("Chart binding changed during diagnostic read");
              if (complete.rows.length + page.rows.length > 512)
                throw new Error("Chart binding exceeds diagnostic row bound");
              complete.rows.push(...page.rows);
            }
            complete.nextOffset = null;
            return complete;
          };
          return {
            input: {
              camera: cameraActions.snapshot(),
              hover: hoverActions.snapshot(),
            },
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
            controllers: [...inspections].flatMap(([owner, state]) =>
              (state.controllers ?? []).map((controller) => ({
                ...controller,
                world: owner.worldReference!,
                session: owner.session,
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
                focusBounds: chartFocusBounds(chart.spec),
                legend: {
                  id: chart.legend.id,
                  world: chart.legend.world,
                  session: chart.legend.client.session,
                  anchor: chart.legend.anchor,
                  surface: chart.legend.surface,
                  extent: chart.legend.extent,
                  unitsPerMetre: CHART_UNITS_PER_METRE,
                  bounds: chart.legend.bounds,
                  center: chart.legend.center,
                  inspection: inspections.get(chart.legend.client)!,
                  title: chart.spec.legendTitle,
                  entries: chart.spec.legendEntries ?? null,
                  scale: chart.spec.legendEntries
                    ? null
                    : {
                        min: CHART_HEIGHT_SCALE.min,
                        max: CHART_HEIGHT_SCALE.max,
                        colors: CHART_HEIGHT_SCALE.colors,
                      },
                },
                binding: await bindingInspection(chart),
              })),
            ),
          };
        },
        dispose() {
          if (!closing) {
            listeners.clear();
            ++cameraGeneration;
            ++hoverGeneration;
            cameraActions.drop();
            hoverActions.drop();
            closing = Promise.all([
              tail.catch(() => {}),
              cameraActions.settled(),
              hoverActions.settled(),
              highlights.settled(),
            ])
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
