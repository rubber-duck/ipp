import type {
  CameraViewMotion,
  PickingWorldClient,
  DataBindingPage,
  GeometryPickHit,
  RowsTable,
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
  readonly source: string;
  readonly sourceIncarnation: bigint;
  readonly bindingIncarnation: bigint;
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
      a.source === b.source &&
      a.sourceIncarnation === b.sourceIncarnation &&
      a.bindingIncarnation === b.bindingIncarnation &&
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
    expandedSamples: false,
    automaticRange: false,
    adaptiveAxes: false,
    smoothChanges: true,
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
    "expandedSamples",
    "automaticRange",
    "adaptiveAxes",
    "smoothChanges",
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
    let hoverPoint: { x: number; y: number } | null = null;
    let feedbackGeneration = 0;
    let feedbackPending = Promise.resolve();
    let closing: Promise<void> | undefined;
    let tail = Promise.resolve();
    let ready = Promise.resolve();
    let initialized = false;
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
      adaptiveAxes: boolean;
    }>(async (marks) =>
      content.highlight(marks.hover, marks.selection, marks.adaptiveAxes),
    );
    const highlight = () =>
      highlights.submit({
        hover,
        selection,
        adaptiveAxes: Boolean(options.adaptiveAxes),
      });
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
      hoverPoint = null;
      return highlight();
    };
    const refreshHover = async () => {
      if (initialized && !closing)
        await hoverActions.submit({
          args: hoverPoint,
          generation: ++hoverGeneration,
        });
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
    /** Charts whose displayed rows can change the current hover or selection. */
    const feedbackCharts = () => {
      const charts = new Set<string>();
      if (selection) charts.add(selection.chart);
      if (hover) charts.add(hover.chart);
      else if (hoverPoint)
        for (const chart of content.charts) charts.add(chart.spec.id);
      return charts;
    };
    const changeView = async <T>(change: () => Promise<T>) => {
      ++feedbackGeneration;
      ++hoverGeneration;
      hoverActions.drop();
      viewTransition = new Promise<void>((resolve) => {
        finishViewTransition = resolve;
      });
      let result: T;
      try {
        // Drain committed feedback before declarations or source lifetimes change.
        await highlights.settled();
        result = await change();
      } finally {
        finishViewTransition!();
        finishViewTransition = undefined;
        viewTransition = undefined;
      }
      await refreshHover();
      const remaining = await content.remainingFeedback(feedbackCharts());
      if (remaining.duration > 0) followFeedback(remaining);
      else {
        // Motion may have finished after the earlier feedback reads.
        await reconcile();
        await refreshHover();
      }
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
        const current = () =>
          chart.source === mark.source &&
          chart.producer.incarnation === mark.sourceIncarnation &&
          page.sourceIncarnation === mark.sourceIncarnation &&
          page.bindingIncarnation === mark.bindingIncarnation &&
          page.availability.reason === "Ready";
        for (;;) {
          if (!current()) return null;
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
      const previousSelection = selection;
      const retainedSelection = await retained(previousSelection);
      if (selection === previousSelection) selection = retainedSelection;
      await highlight();
    };
    const followFeedback = ({
      duration,
      time,
    }: {
      duration: number;
      time: number;
    }) => {
      const generation = ++feedbackGeneration;
      if (duration <= 0) return;
      feedbackPending = (async () => {
        let frame = await content.client.waitForFrame();
        while (!closing && generation === feedbackGeneration) {
          await enqueue(async () => {
            if (closing || generation !== feedbackGeneration) return;
            await reconcile();
            await refreshHover();
            notify();
          });
          if (frame.time >= time + duration) return;
          frame = await content.client.waitForFrame(frame.tick);
        }
      })().catch((error) => {
        if (!closing && generation === feedbackGeneration)
          context.onError?.(
            error instanceof Error ? error : new Error(String(error)),
          );
      });
    };
    // Positional live slots glide after each arrival; follow them like edits.
    const followLiveFeedback = async () => {
      const remaining = await content.remainingFeedback(feedbackCharts());
      if (remaining.duration > 0) followFeedback(remaining);
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
          await refreshHover();
          notify();
          await followLiveFeedback();
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
      sources: ReadonlyMap<string, { source: string; incarnation: bigint }>,
    ): Promise<ChartMark | null> => {
      if (!hit?.row) return null;
      const chart = content.charts.find(
        (chart) =>
          chart.entity === hit.entity &&
          chart.world.id === hit.world.id &&
          chart.world.incarnation === hit.world.incarnation,
      );
      if (!chart) return null;
      const expected = sources.get(chart.spec.id);
      if (!expected) return null;
      let page = await context.canvas.host.datasets.bindingView(
        chart.client.session,
        chart.entity,
      );
      const bindingIncarnation = page.bindingIncarnation;
      const evaluatedTick = page.evaluatedTick;
      const current = () =>
        chart.source === expected.source &&
        chart.producer.incarnation === expected.incarnation &&
        page.sourceIncarnation === expected.incarnation &&
        page.bindingIncarnation === bindingIncarnation &&
        page.evaluatedTick === evaluatedTick &&
        page.availability.reason === "Ready";
      if (!current()) return null;
      let row = page.rows.find((row) => row.id === hit.row!.rowId);
      while (!row && page.nextOffset !== null) {
        page = await context.canvas.host.datasets.bindingView(
          chart.client.session,
          chart.entity,
          { offset: page.nextOffset },
        );
        if (!current()) return null;
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
        source: expected.source,
        sourceIncarnation: expected.incarnation,
        bindingIncarnation,
        values: row.values,
        columns: page.columns.map((column) => column.name),
      };
    };
    const pick = async (args: unknown) => {
      if (args === null) return null;
      const point = args as { x: number; y: number };
      if (!Number.isFinite(point?.x) || !Number.isFinite(point?.y))
        throw new Error("Chart picking requires normalized x/y coordinates");
      // This is an in-memory lifetime fence, without an extra binding read per pick.
      const sources = new Map(
        content.charts.map((chart) => [
          chart.spec.id,
          { source: chart.source, incarnation: chart.producer.incarnation },
        ]),
      );
      const result = await client.query({
        type: "GeometryPickQuery",
        view: { kind: "bound", binding: await binding() },
        x: point.x,
        y: point.y,
        includeViewPlane: false,
      });
      if (!result.ok) throw new Error(result.error);
      return mark(result.hit, sources);
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
        if (args === null) hoverPoint = null;
        else {
          const point = args as { x: number; y: number };
          if (!Number.isFinite(point?.x) || !Number.isFinite(point?.y))
            throw new Error(
              "Chart picking requires normalized x/y coordinates",
            );
          hoverPoint = { x: point.x, y: point.y };
        }
        await hoverActions.submit({
          args: hoverPoint,
          generation: ++hoverGeneration,
        });
      },
      async select(args) {
        selection = await pick(args);
        await highlight();
        // A slot selected mid-glide keeps its panel values current until it settles.
        if (content.mode === "streaming") await followLiveFeedback();
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
            await content.dataSource(
              mode,
              Boolean(options.changed),
              Boolean(options.expandedSamples),
            );
            options = { ...options, dataMode: mode };
            if (mode === "streaming") await feed.prime();
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
          await refreshHover();
          if (content.mode === "streaming") await followLiveFeedback();
        }
      },
      async changeSamples(args) {
        const changed = args === undefined ? true : Boolean(args);
        await changeView(async () => {
          await content.changeSamples(
            changed,
            Boolean(options.expandedSamples),
          );
          options = { ...options, changed };
          await reconcile();
        });
      },
      async expandedSamples(args) {
        if (typeof args !== "boolean")
          throw new Error("Expanded samples requires a boolean");
        await changeView(async () => {
          await content.changeSamples(Boolean(options.changed), args);
          options = { ...options, expandedSamples: args };
          await reconcile();
        });
      },
      async automaticRange(args) {
        if (typeof args !== "boolean")
          throw new Error("Automatic range requires a boolean");
        await changeView(async () => {
          await content.setAutomaticRange(args);
          options = { ...options, automaticRange: args };
          await reconcile();
        });
      },
      async adaptiveAxes(args) {
        if (typeof args !== "boolean")
          throw new Error("Adaptive axes requires a boolean");
        options = { ...options, adaptiveAxes: args };
        await highlight();
      },
      async smoothChanges(args) {
        if (typeof args !== "boolean")
          throw new Error("Smooth changes requires a boolean");
        await changeView(async () => {
          await content.setSmoothChanges(args);
          options = { ...options, smoothChanges: args };
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
      ready = content
        .ready()
        .then(async () => {
          if (!options.smoothChanges) await content.setSmoothChanges(false);
          if (options.changed || options.expandedSamples)
            await content.changeSamples(
              Boolean(options.changed),
              Boolean(options.expandedSamples),
            );
          if (options.automaticRange) await content.setAutomaticRange(true);
          if (options.dataWindow === "time") await content.dataWindow("time");
          if (options.dataMode === "streaming")
            await actions.dataSource!("streaming");
        })
        .then(() => {
          initialized = true;
          // Startup source changes may supersede input already waiting on readiness.
          void refreshHover().catch((error) =>
            context.onError?.(
              error instanceof Error ? error : new Error(String(error)),
            ),
          );
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
            for (const name of [
              "expandedSamples",
              "automaticRange",
              "adaptiveAxes",
              "smoothChanges",
            ])
              if (patch[name] !== undefined && patch[name] !== options[name])
                await actions[name]!(patch[name]);
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
            const limit = Number(context.contract.DATASET_PAGE_ROWS);
            const read = (offset = 0n) =>
              context.canvas.host.datasets.bindingView(
                chart.client.session,
                chart.entity,
                { offset, limit },
              );
            let first = await read();
            for (let attempt = 0; attempt < 5; attempt++) {
              if (first.totalRows > 512n)
                throw new Error("Chart binding exceeds diagnostic row bound");
              if (first.nextOffset === null) return first;
              const offsets = [0n];
              // Gallery projections fit a complete requested page's byte budget.
              for (
                let offset = first.nextOffset;
                offset < first.totalRows;
                offset += BigInt(limit)
              )
                offsets.push(offset);
              // Pages are independent observations. Admit the bounded set together,
              // then merge only if the completed binding frame is exactly identical.
              const pages = await Promise.all(offsets.map(read));
              first = pages[0]!;
              const consistent = pages.every(
                (page, index) =>
                  page.sourceIncarnation === first.sourceIncarnation &&
                  page.bindingIncarnation === first.bindingIncarnation &&
                  page.evaluatedTick === first.evaluatedTick &&
                  page.totalRows === first.totalRows &&
                  page.offset === offsets[index] &&
                  page.nextOffset === (pages[index + 1]?.offset ?? null),
              );
              const rows = pages.flatMap((page) => page.rows);
              if (consistent && BigInt(rows.length) === first.totalRows)
                return { ...first, rows, nextOffset: null };
            }
            throw new Error(
              `Chart ${chart.spec.id} binding kept changing during bounded diagnostic reads`,
            );
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
            adaptiveAxes: Boolean(options.adaptiveAxes),
            automaticRange: Boolean(options.automaticRange),
            expandedSamples: Boolean(options.expandedSamples),
            smoothChanges: Boolean(options.smoothChanges),
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
                labels: [
                  ...((
                    inspections
                      .get(chart.client)!
                      .entities.find((entity) => entity.id === chart.entity)
                      ?.components.find(
                        (component) => "labels" in component.fields,
                      )?.fields.labels as RowsTable | undefined
                  )?.rows.values() ?? []),
                ],
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
            ++feedbackGeneration;
            cameraActions.drop();
            hoverActions.drop();
            closing = Promise.all([
              tail.catch(() => {}),
              cameraActions.settled(),
              hoverActions.settled(),
              highlights.settled(),
              feedbackPending,
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
