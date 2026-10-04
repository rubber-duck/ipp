/** Backend-independent assertions on the real gallery mount, transport and completed frames. */
import type {
  DataBindingPage,
  Inspection,
  RootBinding,
  WorldReference,
} from "@ipp/client";

export interface ChartPose {
  x: number;
  y: number;
  z: number;
  qx: number;
  qy: number;
  qz: number;
  qw: number;
}

export interface GalleryChartsState {
  world: Inspection;
  worldReference: WorldReference;
  session: bigint;
  selectedSystems: readonly string[];
  presentation: RootBinding | null;
  ring: { center: readonly number[]; radius: number };
  camera: {
    entity: bigint;
    navigation: "look" | "orbit";
    transform: ChartPose;
    fields: Record<string, unknown>;
  };
  focus: null | {
    chart: string;
    controller: bigint;
    duration: number;
    from: ChartPose;
    to: ChartPose;
    time: number;
    state: string;
    startTime: number;
  };
  playing: boolean;
  charts: readonly {
    id: string;
    component: string;
    entity: bigint;
    world: WorldReference;
    selectedSystems: readonly string[];
    position: readonly number[];
    center: readonly number[];
    localCenter: readonly number[];
    yaw: number;
    rotation: readonly number[];
    inspection: Inspection;
    anchor: bigint | null;
    controllers: readonly bigint[];
    surface: null | {
      anchor: bigint;
      width: number;
      height: number;
      unitsPerMetre: number;
      extent: readonly number[];
    };
    frame: Record<string, number>;
    binding: DataBindingPage;
  }[];
  hover: ChartRow | null;
  selection: ChartRow | null;
}

export interface ChartRow {
  chart: string;
  entity: bigint;
  world: WorldReference;
  path: readonly unknown[];
  series: number;
  rowId: bigint;
  values: unknown;
}

export interface ChartImage {
  width: number;
  height: number;
  pixels: Uint8Array;
}

export interface GalleryChartsDriver {
  inspect(): Promise<GalleryChartsState>;
  action(name: string, args?: unknown): Promise<unknown>;
  capture(label: string): Promise<ChartImage>;
  record(label: string, value: unknown): Promise<void>;
}

export function chartCheck(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

export function assertChartImage(frame: ChartImage, label: string) {
  let colored = 0;
  for (let i = 0; i < frame.pixels.length; i += 4) {
    const rgb = [...frame.pixels.slice(i, i + 3)];
    if (Math.max(...rgb) > 100 && Math.max(...rgb) - Math.min(...rgb) > 45)
      colored++;
  }
  chartCheck(
    colored > frame.width * frame.height * 0.001,
    `${label}: chart data occupies meaningful coloured pixels (${colored})`,
  );
}

export function assertChartImageChanged(
  before: ChartImage,
  after: ChartImage,
  label: string,
) {
  chartCheck(
    before.width === after.width && before.height === after.height,
    `${label}: completed frame dimensions changed`,
  );
  let changed = 0;
  for (let i = 0; i < before.pixels.length; i += 4)
    if (
      [0, 1, 2].some(
        (channel) =>
          Math.abs(before.pixels[i + channel]! - after.pixels[i + channel]!) >
          20,
      )
    )
      changed++;
  chartCheck(
    changed > before.width * before.height * 0.001,
    `${label}: actual chart pixels changed (${changed})`,
  );
}

function distance(a: ChartPose, b: ChartPose) {
  return Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);
}

function atPose(a: ChartPose, b: ChartPose) {
  const dot = a.qx * b.qx + a.qy * b.qy + a.qz * b.qz + a.qw * b.qw;
  return distance(a, b) < 0.01 && Math.abs(Math.abs(dot) - 1) < 0.001;
}

export function chartTransform(
  state: GalleryChartsState,
  chart: GalleryChartsState["charts"][number],
) {
  const inspection = chart.anchor === null ? chart.inspection : state.world,
    entity = chart.anchor ?? chart.entity;
  const fields = inspection.entities
    .find((item) => item.id === entity)
    ?.components.find((component) => "qx" in component.fields)?.fields;
  chartCheck(fields, `${chart.id}: actual exhibit Transform is available`);
  return fields;
}

export function placePoint(
  fields: Record<string, unknown>,
  point: readonly number[],
) {
  const placed = rotate(
    point.map((value, i) => value * Number(fields[["sx", "sy", "sz"][i]!])),
    ["qx", "qy", "qz", "qw"].map((name) => Number(fields[name])),
  );
  return placed.map((value, i) => value + Number(fields[["x", "y", "z"][i]!]));
}

export function assertCenterCamera(state: GalleryChartsState) {
  chartCheck(
    state.camera.navigation === "look" &&
      Math.hypot(
        state.camera.transform.x - state.ring.center[0]!,
        state.camera.transform.y - state.ring.center[1]!,
        state.camera.transform.z - state.ring.center[2]!,
      ) < 0.001,
    "Center navigation turns in place at the ring center",
  );
}

function assertRingLayout(state: GalleryChartsState) {
  chartCheck(
    state.ring.radius === 34 &&
      state.ring.center.every((value, i) => value === [0, 6, 0][i]),
    "The chart ring has its declared center and radius",
  );
  const exhibits = state.charts.map((chart) => {
    const fields = chartTransform(state, chart),
      center = placePoint(fields, chart.localCenter),
      delta = center.map((value, i) => value - state.ring.center[i]!),
      radius = Math.hypot(delta[0]!, delta[2]!);
    chartCheck(
      center.every((value, i) => Math.abs(value - chart.center[i]!) < 0.001) &&
        chart.position.every(
          (value, i) =>
            Math.abs(value - Number(fields[["x", "y", "z"][i]!])) < 0.001,
        ),
      `${chart.id}: center and origin describe its actual rotated Transform`,
    );
    chartCheck(
      Math.abs(radius - state.ring.radius) < 0.001 &&
        Math.abs(delta[1]!) < 0.001,
      `${chart.id}: actual exhibit center lies on the horizontal circle`,
    );
    const quaternion = ["qx", "qy", "qz", "qw"].map((name) =>
        Number(fields[name]),
      ),
      normal = rotate([0, 0, 1], quaternion);
    chartCheck(
      -(normal[0]! * delta[0]! + normal[2]! * delta[2]!) / radius > 0.999 &&
        Math.abs(normal[1]!) < 0.001,
      `${chart.id}: actual front normal faces inward toward the camera`,
    );
    const size = chart.surface
      ? [chart.surface.width, chart.surface.height, 0]
      : [chart.frame.width!, chart.frame.height!, chart.frame.depth ?? 0];
    return {
      id: chart.id,
      center,
      angle: (Math.atan2(delta[2]!, delta[0]!) + 2 * Math.PI) % (2 * Math.PI),
      extent: Math.hypot(...size) / 2,
    };
  });
  const ordered = [...exhibits].sort((a, b) => a.angle - b.angle);
  for (let i = 0; i < ordered.length; i++) {
    const gap =
      (ordered[(i + 1) % ordered.length]!.angle -
        ordered[i]!.angle +
        2 * Math.PI) %
      (2 * Math.PI);
    chartCheck(
      Math.abs(gap - (2 * Math.PI) / ordered.length) < 0.001,
      "Actual exhibits have equal angular spacing around the ring",
    );
    for (let j = i + 1; j < ordered.length; j++) {
      const a = ordered[i]!,
        b = ordered[j]!;
      chartCheck(
        Math.hypot(...a.center.map((value, axis) => value - b.center[axis]!)) >
          a.extent + b.extent + 1,
        `${a.id}/${b.id}: chart volumes have clear space between them`,
      );
    }
  }
}

function samePresentation(a: GalleryChartsState, b: GalleryChartsState) {
  const left = a.presentation,
    right = b.presentation;
  return (
    left?.output.kind === "camera" &&
    right?.output.kind === "camera" &&
    left.output.entity === right.output.entity &&
    left.output.incarnation === right.output.incarnation &&
    left.generation.host === right.generation.host &&
    left.generation.serial === right.generation.serial
  );
}

export async function waitForCharts(
  driver: GalleryChartsDriver,
  predicate: (state: GalleryChartsState) => boolean,
  label: string,
) {
  const deadline = performance.now() + 15_000;
  for (;;) {
    const state = await driver.inspect();
    if (predicate(state)) return state;
    if (performance.now() >= deadline) {
      await driver.record(`${label}-timeout`, state);
      throw new Error(`${label} did not complete`);
    }
  }
}

export async function focusChart(driver: GalleryChartsDriver, id: string) {
  await driver.action("focus", id);
  const current = await waitForCharts(
    driver,
    (state) => state.focus?.chart === id,
    `focus-${id}-armed`,
  );
  chartCheck(
    current.focus?.duration === 2,
    "Chart focus uses exactly two seconds",
  );
  const target = current.focus.to;
  const done = await waitForCharts(
    driver,
    // Entity and controller inspection pages can name different runtime ticks.
    (state) =>
      state.focus?.chart === id &&
      state.focus.time >= 1.999 &&
      atPose(state.camera.transform, target),
    `focus-${id}-complete`,
  );
  await driver.record(`focus-${id}`, { armed: current, done });
  chartCheck(
    atPose(done.camera.transform, target),
    `${id}: completed focus reaches its camera pose`,
  );
  return done;
}

/** Operations rely on the Host's World/controller clocks; polling never advances simulation. */
export async function exerciseGalleryCharts(driver: GalleryChartsDriver) {
  await driver.action("playback", { playing: false, time: 0 });
  const initial = await driver.inspect();
  await driver.record("charts-initial", initial);
  const center = await driver.capture("ring-center");
  assertChartImage(center, "ring center");
  const ids = [
    "straight",
    "smooth",
    "bars",
    "bins",
    "pie",
    "grid-bars",
    "single-row",
    "height-surface",
    "point-plot",
    "variable-pie",
  ];
  chartCheck(
    initial.charts.length === ids.length &&
      ids.every((id) => initial.charts.some((chart) => chart.id === id)),
    "One scene contains all ten flat and volumetric chart families",
  );
  assertRingLayout(initial);
  assertCenterCamera(initial);
  const flat = initial.charts.filter((chart) => chart.component.endsWith("2d"));
  const spatial = initial.charts.filter((chart) =>
    chart.component.endsWith("3d"),
  );
  chartCheck(
    flat.length === 5 && spatial.length === 5,
    "Five Canvas plots and five spatial plots share the camera scene",
  );
  chartCheck(
    new Set(flat.map((chart) => String(chart.world.id))).size === 5 &&
      flat.every((chart) => chart.world.id !== initial.worldReference.id),
    "Each flat chart owns one distinct Canvas child World",
  );
  chartCheck(
    spatial.every(
      (chart) =>
        chart.world.id === initial.worldReference.id && chart.anchor === null,
    ),
    "Volumetric charts remain in the root World",
  );
  chartCheck(
    initial.charts.every((chart) =>
      chart.inspection.entities.some((entity) => entity.id === chart.entity),
    ),
    "Every chart belongs to its reported runtime World",
  );
  chartCheck(
    !initial.selectedSystems.includes("ipp.canvas") &&
      initial.world.canvas === null,
    "The camera scene does not select Canvas",
  );
  const attachments = initial.world.entities.flatMap((entity) =>
    entity.components
      .filter(
        (component) =>
          "child" in component.fields && "mode" in component.fields,
      )
      .map((component) => ({ entity, fields: component.fields })),
  );
  chartCheck(
    attachments.length === 5 &&
      attachments.every(
        ({ fields }) => Number(fields.mode) === 1 && fields.output === null,
      ),
    "Five SurfaceCanvas attachments present the flat exhibits",
  );
  for (const chart of flat) {
    const attachment = attachments.find(
      ({ entity }) => entity.id === chart.anchor,
    );
    chartCheck(
      attachment &&
        String((attachment.fields.child as WorldReference).id) ===
          String(chart.world.id),
      `${chart.id}: attachment points to its own child World`,
    );
    chartCheck(
      chart.selectedSystems.includes("ipp.canvas") &&
        chart.inspection.canvas?.state.unitsPerMetre === 60 &&
        chart.inspection.canvas.evaluated?.extent[0] === 600 &&
        chart.inspection.canvas.evaluated.extent[1] === 360,
      `${chart.id}: Surface presentation supplies the Canvas extent`,
    );
  }
  const dataControllers = initial.charts.flatMap(
    (chart) =>
      chart.inspection.controllers?.filter((controller) =>
        chart.controllers.includes(controller.id),
      ) ?? [],
  );
  chartCheck(
    dataControllers.length === 10 &&
      dataControllers.every(
        (controller) =>
          controller.state === "paused" && Math.abs(controller.time) < 0.001,
      ),
    "Pause/seek reaches actual data controllers in root and child Worlds",
  );
  let turned = initial;
  for (const [index, yaw] of [0.22, 0.18, -0.25].entries()) {
    await driver.action("navigate", { kind: "rotate", yaw, pitch: 0 });
    const next = await driver.inspect();
    assertCenterCamera(next);
    chartCheck(
      !atPose(next.camera.transform, turned.camera.transform),
      "Successive center turns change orientation without moving the eye",
    );
    chartCheck(
      samePresentation(initial, next),
      "Center turns retain the camera output and root binding",
    );
    await driver.record(`ring-center-turn-${index}`, next);
    turned = next;
  }
  const reset = await focusChart(driver, "center");
  assertCenterCamera(reset);
  chartCheck(
    atPose(reset.camera.transform, initial.camera.transform),
    "Returning to center restores the starting position and direction",
  );
  await driver.action("focus", "bars");
  const middle = await waitForCharts(
    driver,
    (state) =>
      !!state.focus && state.focus.time >= 0.25 && state.focus.time < 1.8,
    "focus-intermediate",
  );
  const focus = middle.focus!;
  chartCheck(
    distance(middle.camera.transform, focus.from) > 0.1 &&
      distance(middle.camera.transform, focus.to) > 0.1,
    "Focus camera samples an intermediate pose",
  );
  const clock = await waitForCharts(
    driver,
    (state) =>
      state.focus?.controller === focus.controller &&
      state.focus.time >= focus.time + 0.15,
    "focus-clock",
  );
  chartCheck(
    Math.abs(
      clock.focus!.time - focus.time - (clock.world.time - middle.world.time),
    ) < 0.1,
    "Focus duration follows the Host World clock",
  );
  await driver.record("focus-intermediate", { middle, clock });
  await driver.capture("charts-focus-intermediate");
  await driver.action("focus", "pie");
  const interrupted = await driver.inspect();
  chartCheck(
    interrupted.focus?.chart === "pie" &&
      interrupted.focus.controller !== focus.controller,
    "Focusing another chart replaces the active flight",
  );
  chartCheck(
    !interrupted.world.controllers?.some(
      (controller) => controller.id === focus.controller,
    ),
    "Interrupted focus releases its runtime controller",
  );
  chartCheck(
    samePresentation(initial, interrupted),
    "Replacing a camera flight preserves its Camera incarnation and root binding",
  );
  await driver.action("navigate", { kind: "rotate", yaw: 0.2, pitch: 0.1 });
  const manual = await driver.inspect();
  chartCheck(manual.focus === null, "A free camera gesture cancels focus");
  chartCheck(
    manual.camera.navigation === "orbit",
    "A focused chart retains orbit navigation after interrupting its flight",
  );
  chartCheck(
    samePresentation(initial, manual),
    "Manual cancellation preserves its Camera incarnation and root binding",
  );
  chartCheck(
    !atPose(manual.camera.transform, interrupted.camera.transform),
    "Free navigation changes the camera from its sampled pose",
  );

  await driver.action("focus", "overview");
  await waitForCharts(
    driver,
    (state) => (state.focus?.time ?? 0) >= 1.999,
    "angled-overview-ready",
  );
  assertChartImage(await driver.capture("ring-whole"), "whole ring");
  await driver.action("navigate", { kind: "rotate", yaw: 0.25, pitch: 0.12 });
  assertChartImage(
    await driver.capture("charts-free-angle"),
    "angled overview",
  );

  for (const id of ids) {
    const current = await focusChart(driver, id);
    chartCheck(
      current.session === initial.session &&
        current.worldReference.id === initial.worldReference.id,
      "Focusing preserves the root camera World and session",
    );
    const focused = await driver.capture(`charts-focus-${id}`);
    const measurement = assertFocusedChartImage(focused, id);
    await driver.record(`charts-focus-${id}-pixels`, measurement);
  }
  await focusChart(driver, "bars");
  const before = await driver.capture("charts-data-phase-zero");
  await driver.action("playback", { playing: false, time: 2 });
  const after = await driver.capture("charts-data-phase-two");
  assertChartImageChanged(before, after, "Host animation seek");
  const paused = await driver.inspect();
  chartCheck(!paused.playing, "Playback pauses the chart visuals");
  for (const chart of paused.charts) {
    const original = initial.charts.find((item) => item.id === chart.id)!;
    chartCheck(
      chart.binding.rows.some((row, index) =>
        row.values.some((value, column) => {
          const baseline = original.binding.rows[index]?.values[column];
          return (
            value.valid &&
            baseline?.valid &&
            value.value.kind === "f32" &&
            baseline.value.kind === "f32" &&
            Math.abs(value.value.value - baseline.value.value) > 0.01
          );
        }),
      ),
      `${chart.id}: animation changes actual evaluated chart data`,
    );
  }
  await driver.action("playback", { playing: true });
  const advanced = await waitForCharts(
    driver,
    (state) => state.world.time > paused.world.time + 1,
    "data-playing",
  );
  chartCheck(advanced.playing, "Chart playback resumes");
  chartCheck(
    advanced.charts.every((chart) =>
      chart.inspection.controllers
        ?.filter((controller) => chart.controllers.includes(controller.id))
        .every((controller) => controller.state === "playing"),
    ),
    "Resume reaches root and Canvas child data controllers",
  );
  await driver.action("playback", { playing: false });
  const pausedAgain = await driver.inspect();
  const stable = await driver.capture("charts-data-paused");
  assertChartImageChanged(after, stable, "Resumed Host animation");
  const later = await driver.capture("charts-data-paused-again");
  chartCheck(
    stable.pixels.every(
      (pixel, index) => Math.abs(pixel - later.pixels[index]!) <= 1,
    ),
    "Paused chart visuals retain identical completed pixels",
  );
  await driver.record("charts-animation", { paused, advanced, pausedAgain });
  await driver.action("playback", { playing: false, time: 0 });
  await focusChart(driver, "grid-bars");
  const gridBefore = await driver.capture("charts-grid-data-phase-zero");
  await driver.action("playback", { playing: false, time: 2 });
  const gridAfter = await driver.capture("charts-grid-data-phase-two");
  assertChartImageChanged(gridBefore, gridAfter, "Spatial bar Host animation");
}

function rotate(vector: readonly number[], quaternion: readonly number[]) {
  const [x, y, z] = vector as [number, number, number];
  const [qx, qy, qz, qw] = quaternion as [number, number, number, number];
  const t = [
    2 * (qy * z - qz * y),
    2 * (qz * x - qx * z),
    2 * (qx * y - qy * x),
  ];
  return [
    x + qw * t[0]! + qy * t[2]! - qz * t[1]!,
    y + qw * t[1]! + qz * t[0]! - qx * t[2]!,
    z + qw * t[2]! + qx * t[1]! - qy * t[0]!,
  ];
}

/** Independently project a baseline bar interior, without reading derived Plot geometry. */
export function baselineBarPointer(state: GalleryChartsState, aspect: number) {
  const chart = state.charts.find((chart) => chart.id === "bars");
  chartCheck(chart, "Missing bars chart");
  const entity = chart.inspection.entities.find(
    (entity) => entity.id === chart.entity,
  );
  chartCheck(entity, "Missing bars entity");
  const frame = entity.components.find(
    (component) => "padding_left" in component.fields,
  )?.fields;
  const anchor = state.world.entities.find(
    (entity) => entity.id === chart.anchor,
  );
  const object = anchor?.components.find(
    (component) => "qx" in component.fields,
  )?.fields;
  chartCheck(frame && object, "Bar frame and Transform are available");
  const width = Number(frame.width),
    height = Number(frame.height);
  const x =
    Number(frame.padding_left) +
    ((2 - Number(frame.min_x)) / (Number(frame.max_x) - Number(frame.min_x))) *
      (width - Number(frame.padding_left) - Number(frame.padding_right));
  const y =
    height -
    Number(frame.padding_bottom) -
    (32.5 / (Number(frame.max_y) - Number(frame.min_y))) *
      (height - Number(frame.padding_top) - Number(frame.padding_bottom));
  const density = Number(chart.surface?.unitsPerMetre);
  const point = rotate(
    [
      ((x - width / 2) / density) * Number(object.sx),
      ((height / 2 - y) / density) * Number(object.sy),
      0,
    ],
    [
      Number(object.qx),
      Number(object.qy),
      Number(object.qz),
      Number(object.qw),
    ],
  );
  return projectChartPoint(
    state,
    point.map(
      (value, index) => value + Number(object[["x", "y", "z"][index]!]),
    ),
    aspect,
  );
}

export function projectChartPoint(
  state: GalleryChartsState,
  world: readonly number[],
  aspect: number,
) {
  const camera = state.camera.transform;
  const view = rotate(
    [world[0]! - camera.x, world[1]! - camera.y, world[2]! - camera.z],
    [-camera.qx, -camera.qy, -camera.qz, camera.qw],
  );
  const halfHeight =
    Number(state.camera.fields.projection) === 1
      ? Number(state.camera.fields.ortho_height) / 2
      : -view[2]! * Math.tan(Number(state.camera.fields.fov_y) / 2);
  return {
    x: 0.5 + view[0]! / (2 * halfHeight * aspect),
    y: 0.5 - view[1]! / (2 * halfHeight),
  };
}

/** Grid row six is independently known to be (x1,z1,y85) in fixed 10×5×10 axes. */
export function baselineGridPointer(state: GalleryChartsState, aspect: number) {
  const chart = state.charts.find((chart) => chart.id === "grid-bars");
  chartCheck(chart, "Missing grid bars");
  return projectChartPoint(
    state,
    placePoint(chartTransform(state, chart), [3.75, 4.25, 5]),
    aspect,
  );
}

/** The rotated single-row exhibit's tallest bar is source row two: (x1,y50,z0). */
export function baselineSingleRowPointer(
  state: GalleryChartsState,
  aspect: number,
) {
  const chart = state.charts.find((chart) => chart.id === "single-row");
  chartCheck(chart, "Missing single-row chart");
  return projectChartPoint(
    state,
    placePoint(chartTransform(state, chart), [3.75, 2.5, 10 / 6]),
    aspect,
  );
}

export async function exerciseChartRow(
  driver: GalleryChartsDriver,
  aspect: number,
) {
  await driver.action("playback", { playing: false, time: 0 });
  const state = await focusChart(driver, "bars");
  const point = baselineBarPointer(state, aspect);
  await driver.action("hover", point);
  const hovered = await driver.inspect();
  chartCheck(
    hovered.hover?.chart === "bars" && String(hovered.hover.rowId) === "2",
    "Native chart pointer query retains source row two",
  );
  await driver.action("select", point);
  const selected = await driver.inspect();
  chartCheck(
    selected.selection?.chart === "bars" &&
      String(selected.selection.rowId) === "2",
    "Native selection retains source row two",
  );
  chartCheck(
    String(selected.selection.world.id) ===
      String(state.charts.find((chart) => chart.id === "bars")!.world.id) &&
      selected.selection.path.length === 1 &&
      selected.selection.entity ===
        state.charts.find((chart) => chart.id === "bars")!.entity,
    "Native selection targets the Canvas chart with its World-qualified path",
  );
  await driver.record("charts-source-row", { point, hovered, selected });
  await driver.capture("charts-selected-source-row");
  const grid = await focusChart(driver, "grid-bars");
  const gridPoint = baselineGridPointer(grid, aspect);
  await driver.action("hover", gridPoint);
  const gridHovered = await driver.inspect();
  chartCheck(
    gridHovered.hover?.chart === "grid-bars" &&
      String(gridHovered.hover.rowId) === "6",
    "Volumetric source row hover shares the camera picking domain",
  );
  await driver.action("select", gridPoint);
  const gridSelected = await driver.inspect();
  chartCheck(
    gridSelected.selection?.chart === "grid-bars" &&
      String(gridSelected.selection.rowId) === "6",
    "Volumetric selection preserves source row six",
  );
  chartCheck(
    String(gridSelected.selection.world.id) ===
      String(grid.worldReference.id) &&
      gridSelected.selection.path.length === 0,
    "Spatial row selection belongs to the camera World with no Surface path",
  );
  await driver.capture("charts-selected-volumetric-row");
  await driver.record("charts-volumetric-row", {
    gridPoint,
    gridHovered,
    gridSelected,
  });
  await driver.action("clearSelection");
  chartCheck(
    (await driver.inspect()).selection === null,
    "Selection clears explicitly",
  );
  const rotated = await focusChart(driver, "single-row");
  const rotatedChart = rotated.charts.find(
    (chart) => chart.id === "single-row",
  )!;
  chartCheck(
    Math.abs(Number(chartTransform(rotated, rotatedChart).qy)) > 0.1,
    "The additional spatial row fixture has a nonidentity yaw",
  );
  const rotatedPoint = baselineSingleRowPointer(rotated, aspect);
  await driver.action("hover", rotatedPoint);
  const rotatedHover = await driver.inspect();
  chartCheck(
    rotatedHover.hover?.chart === "single-row" &&
      String(rotatedHover.hover.rowId) === "2",
    "Rotated spatial hover preserves source row two",
  );
  await driver.action("select", rotatedPoint);
  const rotatedSelection = await driver.inspect();
  chartCheck(
    rotatedSelection.selection?.chart === "single-row" &&
      String(rotatedSelection.selection.rowId) === "2" &&
      rotatedSelection.selection.world.id === rotated.worldReference.id &&
      rotatedSelection.selection.path.length === 0,
    "Rotated spatial selection preserves its source row and root World",
  );
  await driver.record("ring-rotated-spatial-row", {
    rotatedPoint,
    rotatedHover,
    rotatedSelection,
  });
  await driver.capture("ring-selected-rotated-spatial-row");
  await driver.action("clearSelection");
}

/** Focus places the selected chart in this central plot region; colored ink excludes white labels. */
export function assertFocusedChartImage(frame: ChartImage, label: string) {
  const box = [0.26, 0.26, 0.74, 0.74];
  let colored = 0;
  for (
    let y = Math.floor(frame.height * box[1]!);
    y < frame.height * box[3]!;
    y++
  )
    for (
      let x = Math.floor(frame.width * box[0]!);
      x < frame.width * box[2]!;
      x++
    ) {
      const at = (y * frame.width + x) * 4;
      const rgb = [...frame.pixels.slice(at, at + 3)];
      if (Math.max(...rgb) > 100 && Math.max(...rgb) - Math.min(...rgb) > 45)
        colored++;
    }
  chartCheck(
    colored > frame.width * frame.height * 0.0007,
    `${label}: focused data occupies the central chart region (${colored})`,
  );
  return { box, colored, width: frame.width, height: frame.height };
}
