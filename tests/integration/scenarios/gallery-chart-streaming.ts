/** Optional live gallery data, observed through actual dataset pages and completed frames. */
import type { DatasetPage } from "@ipp/client";
import {
  assertChartImage,
  assertChartImageChanged,
  chartCheck,
  chartTransform,
  placePoint,
  projectChartPoint,
  focusChart,
  type GalleryChartsDriver,
  type GalleryChartsState,
} from "./gallery-charts.js";

type Window =
  | { kind: "count"; count: bigint }
  | {
      kind: "range";
      column: string;
      width: number;
      anchor:
        | { kind: "latest" }
        | { kind: "supplied"; value: { value: number } };
    };
export interface StreamingChartsState extends GalleryChartsState {
  data: {
    mode: "buffer" | "streaming";
    window: "count" | "time";
    feed: {
      playing: boolean;
      status: string;
      sequence: number;
      inFlight: boolean;
      error: string | null;
    };
    sources: DatasetPage[];
  };
  charts: readonly (GalleryChartsState["charts"][number] & {
    source: string;
    sourceKind: "buffer" | "streaming";
    bindingComponent: string;
    windows: readonly Window[];
  })[];
}
export interface StreamingChartsDriver extends GalleryChartsDriver {
  inspect(): Promise<StreamingChartsState>;
}

async function until(
  driver: StreamingChartsDriver,
  ready: (state: StreamingChartsState) => boolean,
  label: string,
) {
  const deadline = performance.now() + 20_000;
  for (;;) {
    const state = await driver.inspect();
    chartCheck(
      !state.data.feed.error,
      `Live feed failed: ${state.data.feed.error}`,
    );
    if (ready(state)) return state;
    if (performance.now() >= deadline) {
      await driver.record(`${label}-timeout`, state);
      throw new Error(`${label}: dataset observation did not settle`);
    }
  }
}

const ids = (rows: readonly { id: bigint }[]) =>
  rows.map((row) => String(row.id));
const equalIds = (a: readonly { id: bigint }[], b: readonly { id: bigint }[]) =>
  JSON.stringify(ids(a)) === JSON.stringify(ids(b));
function raw(
  source: DatasetPage,
  row: DatasetPage["rows"][number],
  column: string,
) {
  const index = source.schema.findIndex((item) => item.name === column);
  const value = row.values[index];
  chartCheck(
    value && value.kind === "f32",
    `Source column ${column} is numeric`,
  );
  return value.value;
}

function assertWindows(state: StreamingChartsState) {
  for (const source of state.data.sources) {
    const charts = state.charts.filter((chart) => chart.source === source.name);
    chartCheck(
      source.kind === "streaming" && charts.length,
      "Actual source is a consumed live dataset",
    );
    chartCheck(
      source.nextOffset === null,
      "Diagnostics include the complete retained source",
    );
    const union = new Set<string>();
    for (const chart of charts) {
      chartCheck(
        chart.sourceKind === "streaming" &&
          chart.bindingComponent === "StreamingDataSourceBinding" &&
          chart.binding.availability.reason === "Ready" &&
          String(chart.binding.sourceIncarnation) ===
            String(source.incarnation),
        `${chart.id}: live binding resolves the actual source incarnation`,
      );
      let expected = [...source.rows];
      for (const window of chart.windows) {
        if (window.kind === "count")
          expected = expected.slice(-Number(window.count));
        else {
          const anchor =
            window.anchor.kind === "latest"
              ? Math.max(
                  ...source.rows.map((row) => raw(source, row, window.column)),
                )
              : window.anchor.value.value;
          expected = expected.filter((row) => {
            const value = raw(source, row, window.column);
            return value >= anchor - window.width && value <= anchor;
          });
        }
      }
      chartCheck(
        equalIds(chart.binding.rows, expected),
        `${chart.id}: binding membership matches authored windows over retained raw rows`,
      );
      chartCheck(
        chart.binding.nextOffset === null,
        `${chart.id}: all evaluated rows are observed`,
      );
      for (const row of chart.binding.rows) union.add(String(row.id));
    }
    chartCheck(
      union.size === source.rows.length &&
        source.rows.every((row) => union.has(String(row.id))) &&
        Number(source.memory.retainedRows) === source.rows.length,
      "Source retains exactly the union requested across root and child Worlds",
    );
  }
}

function assertSourceMode(
  state: StreamingChartsState,
  mode: "buffer" | "streaming",
) {
  chartCheck(state.data.mode === mode, `Gallery selects ${mode} sources`);
  chartCheck(
    state.charts.every(
      (chart) =>
        chart.sourceKind === mode &&
        chart.bindingComponent ===
          (mode === "buffer"
            ? "BufferDataSourceBinding"
            : "StreamingDataSourceBinding") &&
        chart.binding.availability.reason === "Ready",
    ),
    "All ten chart families use the selected real binding kind",
  );
  for (const chart of state.charts) {
    const source = state.data.sources.find(
      (source) => source.name === chart.source,
    );
    chartCheck(
      source?.kind === mode &&
        String(source.incarnation) === String(chart.binding.sourceIncarnation),
      `${chart.id}: selected binding reads the actual ${mode} source incarnation`,
    );
  }
}

async function pause(driver: StreamingChartsDriver) {
  await driver.action("streamPlayback", false);
  let lastFailure: string | undefined;
  try {
    return await until(
      driver,
      (state) => {
        if (state.data.feed.playing || state.data.feed.inFlight) return false;
        // Sources and binding pages are independent observations. Once ingress drains,
        // wait for every World to publish the same retained data before comparing them.
        try {
          assertWindows(state);
          return true;
        } catch (error) {
          lastFailure = error instanceof Error ? error.message : String(error);
          return false;
        }
      },
      "stream-paused-and-evaluated",
    );
  } catch (error) {
    await driver.record("stream-window-readiness-failure", { lastFailure });
    throw error;
  }
}

function streamingLinePointer(state: StreamingChartsState, aspect: number) {
  const chart = state.charts.find((chart) => chart.id === "straight");
  chartCheck(chart?.binding.rows.length, "Live straight samples exist");
  const frame = chart.inspection.entities
    .find((entity) => entity.id === chart.entity)
    ?.components.find(
      (component) => "padding_left" in component.fields,
    )?.fields;
  chartCheck(
    frame && chart.surface,
    "Actual line frame and Surface mapping are available",
  );
  const value = (row: (typeof chart.binding.rows)[number], column: string) => {
    const result =
      row.values[
        chart.binding.columns.findIndex((item) => item.name === column)
      ];
    chartCheck(
      result?.valid && result.value.kind === "f32",
      `Live line ${column} is valid numeric data`,
    );
    return result.value.value;
  };
  const row = chart.binding.rows[0]!;
  // The line's automatic horizontal extent is the observed sample extent;
  // vertical bounds remain the authored signal range. No Plot geometry is read.
  chartCheck(
    frame.automatic_x === true && frame.automatic_y === false,
    "Live lines use sample X bounds and fixed Y bounds",
  );
  const xs = chart.binding.rows.map((row) => value(row, "x")),
    min = Math.min(...xs),
    max = Math.max(...xs),
    width = Number(frame.width),
    height = Number(frame.height);
  const x =
    Number(frame.padding_left) +
    ((value(row, "x") - min) / (max - min)) *
      (width - Number(frame.padding_left) - Number(frame.padding_right));
  const y =
    height -
    Number(frame.padding_bottom) -
    ((value(row, "y") - Number(frame.min_y)) /
      (Number(frame.max_y) - Number(frame.min_y))) *
      (height - Number(frame.padding_top) - Number(frame.padding_bottom));
  return {
    ...projectChartPoint(
      state,
      placePoint(chartTransform(state, chart), [
        (x - width / 2) / chart.surface.unitsPerMetre,
        (height / 2 - y) / chart.surface.unitsPerMetre,
        0,
      ]),
      aspect,
    ),
    rowId: row.id,
  };
}

export function streamingPointPointer(
  state: StreamingChartsState,
  aspect: number,
) {
  const chart = state.charts.find((chart) => chart.id === "point-plot");
  chartCheck(chart, "Point chart exists");
  const source = state.data.sources.find(
    (source) => source.name === chart.source,
  );
  chartCheck(source?.rows.length, "Actual live point rows exist");
  const row = source.rows.at(-1)!;
  const frame = chart.inspection.entities
    .find((entity) => entity.id === chart.entity)
    ?.components.find(
      (component) => "depth" in component.fields && "min_x" in component.fields,
    )?.fields;
  chartCheck(frame, "Actual spatial frame is available");
  const local = ["x", "y", "z"].map((axis, index) => {
    const min = Number(frame[`min_${axis}`]),
      max = Number(frame[`max_${axis}`]);
    return (
      ((raw(source, row, axis) - min) / (max - min)) *
      Number(frame[["width", "height", "depth"][index]!])
    );
  });
  return {
    ...projectChartPoint(
      state,
      placePoint(chartTransform(state, chart), local),
      aspect,
    ),
    rowId: row.id,
  };
}

/** Fresh environment; client feed and visual animation are independent of the Host clock. */
export async function exerciseStreamingCharts(
  driver: StreamingChartsDriver,
  aspect: number,
) {
  await driver.action("playback", { playing: false, time: 0 });
  const initial = await driver.inspect();
  assertSourceMode(initial, "buffer");
  const original = {
    world: String(initial.worldReference.id),
    camera: String(initial.camera.entity),
    charts: initial.charts.map((chart) => [
      chart.id,
      String(chart.entity),
      String(chart.world.id),
    ]),
  };
  await driver.action("dataSource", "streaming");
  await until(
    driver,
    (state) =>
      state.charts.find((chart) => chart.id === "smooth")!.binding.rows
        .length === 16,
    "count-window-filled",
  );
  let current = await pause(driver);
  assertSourceMode(current, "streaming");
  assertWindows(current);
  const straight = current.charts.find((chart) => chart.id === "straight")!,
    smooth = current.charts.find((chart) => chart.id === "smooth")!;
  chartCheck(
    straight.source === smooth.source &&
      straight.world.id !== smooth.world.id &&
      straight.binding.rows.length === 8 &&
      smooth.binding.rows.length === 16,
    "Different Canvas Worlds share eight/sixteen row windows on one source",
  );
  await driver.record("stream-count-windows", current);
  await focusChart(driver, "straight");
  current = await driver.inspect();
  const linePoint = streamingLinePointer(current, aspect);
  await driver.action("select", { x: linePoint.x, y: linePoint.y });
  const lineSelected = await until(
    driver,
    (state) => state.selection?.chart === "straight",
    "shared-line-selected",
  );
  chartCheck(
    String(lineSelected.selection?.rowId) === String(linePoint.rowId) &&
      lineSelected.selection?.path.length === 1 &&
      String(lineSelected.selection.world.id) === String(straight.world.id) &&
      String(lineSelected.selection.entity) === String(straight.entity),
    "Real flat pick selects a row in the shorter shared-source window",
  );
  await driver.action("streamPlayback", true);
  const sharedExpiry = await until(
    driver,
    (state) => {
      const short = state.charts.find((chart) => chart.id === "straight")!,
        long = state.charts.find((chart) => chart.id === "smooth")!,
        source = state.data.sources.find(
          (source) => source.name === short.source,
        )!;
      const present = (rows: readonly { id: bigint }[]) =>
        rows.some((row) => String(row.id) === String(linePoint.rowId));
      return (
        !present(short.binding.rows) &&
        present(long.binding.rows) &&
        present(source.rows) &&
        state.selection === null
      );
    },
    "shared-short-window-selection-expires",
  );
  await pause(driver);
  await driver.record("stream-shared-window-selection-expiry", {
    linePoint,
    lineSelected,
    sharedExpiry,
  });

  await focusChart(driver, "bars");
  const flatBefore = await driver.capture("stream-flat-before");
  assertChartImage(flatBefore, "live flat chart");
  await focusChart(driver, "point-plot");
  current = await driver.inspect();
  const spatialBefore = await driver.capture("stream-spatial-before");
  assertChartImage(spatialBefore, "live spatial chart");
  const point = streamingPointPointer(current, aspect);
  await driver.action("select", { x: point.x, y: point.y });
  const selected = await until(
    driver,
    (state) => state.selection?.chart === "point-plot",
    "live-point-selected",
  );
  chartCheck(
    String(selected.selection?.rowId) === String(point.rowId) &&
      String(selected.selection?.world.id) === original.world &&
      selected.selection?.path.length === 0,
    "Real spatial pick returns the live source row and root World",
  );
  await driver.capture("stream-selected-point");
  await driver.record("stream-source-row-selected", { point, selected });
  const beforeIds = new Map(
    current.charts.map((chart) => [chart.id, ids(chart.binding.rows)]),
  );
  const sequence = current.data.feed.sequence;
  await driver.action("streamPlayback", true);
  await until(
    driver,
    (state) =>
      state.data.feed.sequence >= sequence + 4 &&
      !state.charts
        .find((chart) => chart.id === "point-plot")!
        .binding.rows.some((row) => String(row.id) === String(point.rowId)) &&
      state.selection === null,
    "live-row-expired",
  );
  current = await pause(driver);
  assertWindows(current);
  chartCheck(
    current.charts.every(
      (chart) =>
        JSON.stringify(ids(chart.binding.rows)) !==
        JSON.stringify(beforeIds.get(chart.id)),
    ),
    "Actual source row identities advance for every chart family",
  );
  chartCheck(
    current.charts.every(
      (chart) =>
        chart.controllers.length > 0 &&
        chart.controllers.every((id) => {
          const controller = chart.inspection.controllers?.find(
            (item) => item.id === id,
          );
          return (
            controller?.state === "paused" && Math.abs(controller.time) < 0.001
          );
        }),
    ),
    "Rendered changes come from data ingress while visual animation stays at phase zero",
  );
  const spatialAfter = await driver.capture("stream-spatial-after");
  assertChartImageChanged(spatialBefore, spatialAfter, "live spatial arrivals");
  await focusChart(driver, "bars");
  const flatAfter = await driver.capture("stream-flat-after");
  assertChartImageChanged(flatBefore, flatAfter, "live flat arrivals");
  await driver.record("stream-arrivals-and-expiry", current);
  const beforeTimeIds = ids(
    current.data.sources.find((source) => source.name === straight.source)!
      .rows,
  );
  await driver.action("dataWindow", "time");
  current = await pause(driver);
  assertWindows(current);
  chartCheck(
    current.data.window === "time" &&
      current.charts.every((chart) =>
        chart.windows.some((window) => window.kind === "range"),
      ),
    "Time control configures real range windows",
  );
  const timeSequence = current.data.feed.sequence;
  await driver.action("streamPlayback", true);
  await until(
    driver,
    (state) =>
      state.data.feed.sequence >= timeSequence + 10 &&
      state.data.sources
        .find((source) => source.name === straight.source)!
        .rows.every((row) => !beforeTimeIds.includes(String(row.id))),
    "time-window-advances",
  );
  current = await pause(driver);
  assertWindows(current);
  const retained = current.data.sources.find(
    (source) => source.name === straight.source,
  )!;
  chartCheck(
    retained.rows.length > 0 &&
      retained.rows.length <= 64 &&
      retained.rows.every((row) => !beforeTimeIds.includes(String(row.id))),
    "Time windows retain bounded new samples and expire previously observed history",
  );
  const rangeRows = retained.rows;
  await driver.record("stream-time-windows", current);
  await driver.action("dataWindow", "count");
  current = await pause(driver);
  assertWindows(current);
  const retainedIds = ids(
    current.data.sources.find((source) => source.name === straight.source)!
      .rows,
  );
  chartCheck(
    retainedIds.length <= 16 &&
      retainedIds.every((id) => ids(rangeRows).includes(id)) &&
      retainedIds.every((id) => !beforeTimeIds.includes(id)),
    "Removing the range exposes only surviving history, never expired source rows",
  );
  await driver.action("dataWindow", "time");
  current = await pause(driver);
  assertWindows(current);
  chartCheck(
    equalIds(
      current.data.sources.find((source) => source.name === straight.source)!
        .rows,
      rangeRows.filter((row) => retainedIds.includes(String(row.id))),
    ),
    "Widening windows never resurrects expired samples",
  );
  await driver.record("stream-window-no-resurrection", current);
  await driver.action("dataSource", "buffer");
  current = await driver.inspect();
  assertSourceMode(current, "buffer");
  chartCheck(
    !current.data.feed.playing && !current.data.feed.inFlight,
    "Buffer mode stops and drains the synthetic feed",
  );
  chartCheck(
    current.selection === null && current.hover === null,
    "Source switches clear stale feedback",
  );
  chartCheck(
    JSON.stringify({
      world: String(current.worldReference.id),
      camera: String(current.camera.entity),
      charts: current.charts.map((chart) => [
        chart.id,
        String(chart.entity),
        String(chart.world.id),
      ]),
    }) === JSON.stringify(original),
    "Fixed/live/fixed changes preserve chart entities, Worlds and camera",
  );
  await driver.capture("stream-returned-fixed");
  await driver.record("stream-fixed-live-fixed", current);
  return {
    sourceNames: selected.data.sources.map((source) => source.name),
    original,
  };
}
