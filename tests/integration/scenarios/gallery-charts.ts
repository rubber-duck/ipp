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
  input: {
    camera: ChartInputLane;
    hover: ChartInputLane;
  };
  charts: readonly {
    id: string;
    title: string;
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
    surface: null | {
      anchor: bigint;
      component: "CylinderSurface";
      width: number;
      height: number;
      curvature: number;
      unitsPerMetre: number;
      extent: readonly number[];
    };
    frame: Record<string, number>;
    focusBounds: { min: readonly number[]; max: readonly number[] };
    legend: {
      id: string;
      world: WorldReference;
      session: bigint;
      anchor: bigint;
      surface: "CylinderSurface" | "FlatSurface";
      extent: readonly number[];
      unitsPerMetre: number;
      bounds: readonly number[];
      center: readonly number[];
      inspection: Inspection;
      title: string;
      entries:
        | null
        | readonly { id: string; label: string; color: readonly number[] }[];
      scale: null | {
        min: number;
        max: number;
        colors: readonly (readonly number[])[];
      };
    };
    binding: DataBindingPage;
  }[];
  hover: ChartRow | null;
  selection: ChartRow | null;
}

export interface ChartInputLane {
  inFlight: number;
  pending: number;
  submitted: number;
  executed: number;
  coalesced: number;
  dropped: number;
  maxInFlight: number;
  maxPending: number;
}

export interface ChartRow {
  chart: string;
  entity: bigint;
  world: WorldReference;
  path: readonly unknown[];
  series: number;
  rowId: bigint;
  values: DataBindingPage["rows"][number]["values"];
  columns: readonly string[];
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

const categoryColors = [
  [0.03, 0.35, 0.88, 1],
  [1, 0.36, 0.04, 1],
  [0.08, 0.68, 0.23, 1],
  [0.7, 0.1, 0.5, 1],
] as const;

/** Independent linear interpolation of the fixture's 0/2/4 metre scale stops. */
export function expectedHeightColor(height: number) {
  const stops = [
    [0.02, 0.06, 0.25, 1],
    [0.03, 0.48, 0.4, 1],
    [0.96, 0.68, 0.12, 1],
  ];
  const value = Math.max(0, Math.min(4, height)),
    start = value <= 2 ? 0 : 1,
    t = (value - start * 2) / 2;
  return stops[start]!.map(
    (channel, index) => channel + (stops[start + 1]![index]! - channel) * t,
  );
}

function srgbColor(color: readonly number[]) {
  return color
    .slice(0, 3)
    .map((value) =>
      Math.round(
        255 *
          (value <= 0.0031308
            ? 12.92 * value
            : 1.055 * value ** (1 / 2.4) - 0.055),
      ),
    );
}

function probeChartColor(
  frame: ChartImage,
  point: { x: number; y: number },
  color: readonly number[],
  label: string,
) {
  const x = Math.round(point.x * frame.width),
    y = Math.round(point.y * frame.height),
    expected = srgbColor(color);
  chartCheck(
    x > 2 && x < frame.width - 3 && y > 2 && y < frame.height - 3,
    `${label}: independently projected color probe is visible in the focused viewport`,
  );
  const samples: number[][] = [];
  for (let py = y - 1; py <= y + 1; py++)
    for (let px = x - 1; px <= x + 1; px++)
      samples.push([
        ...frame.pixels.slice(
          (py * frame.width + px) * 4,
          (py * frame.width + px) * 4 + 3,
        ),
      ]);
  chartCheck(
    samples.some((rgb) =>
      rgb.every((value, channel) => Math.abs(value - expected[channel]!) < 20),
    ),
    `${label}: completed pixels match linear color ${color} (expected ${expected}, actual ${JSON.stringify(samples)})`,
  );
  return { point, expected, samples };
}

function legendFields(
  chart: GalleryChartsState["charts"][number],
  id: string,
  field: string,
) {
  const value = chart.legend.inspection.entities
    .find((entity) => entity.metadata.symbolicId === id)
    ?.components.find((component) => field in component.fields)?.fields;
  chartCheck(
    value,
    `${chart.id}: actual legend ${id}/${field} is acknowledged`,
  );
  return value;
}

function legendPoint(
  state: GalleryChartsState,
  chart: GalleryChartsState["charts"][number],
  x: number,
  y: number,
  aspect: number,
) {
  const legend = chart.legend;
  if (chart.surface)
    return projectChartPoint(
      state,
      placePoint(
        chartTransform(state, chart),
        chartSurfaceLocalPoint(
          state,
          x / legend.unitsPerMetre - chart.surface.width / 2,
          chart.surface.height / 2 - y / legend.unitsPerMetre,
        ),
      ),
      aspect,
    );
  const anchor = state.world.entities
    .find((entity) => entity.id === legend.anchor)
    ?.components.find((component) => "qx" in component.fields)?.fields;
  chartCheck(
    anchor,
    `${chart.id}: spatial legend has an actual local Transform`,
  );
  return projectChartPoint(
    state,
    placePoint(
      chartTransform(state, chart),
      placePoint(anchor, [
        (x - legend.extent[0]! / 2) / legend.unitsPerMetre,
        (legend.extent[1]! / 2 - y) / legend.unitsPerMetre,
        0,
      ]),
    ),
    aspect,
  );
}

function assertNumericLegendPixels(
  frame: ChartImage,
  state: GalleryChartsState,
  chart: GalleryChartsState["charts"][number],
) {
  const legend = chart.legend,
    root = legendFields(chart, legend.id, "scale_x"),
    first = legendFields(chart, `${legend.id}/scale/0`, "scale_x"),
    last = legendFields(chart, `${legend.id}/scale/31`, "scale_x"),
    firstBox = legendFields(chart, `${legend.id}/scale/0`, "width"),
    lastBox = legendFields(chart, `${legend.id}/scale/31`, "width"),
    left = Number(root.x) + Number(first.x),
    width = Number(firstBox.width),
    top = Number(root.y) + Number(first.y),
    bottom = Number(root.y) + Number(last.y) + Number(lastBox.height),
    aspect = frame.width / frame.height;
  const project = (x: number, fraction: number) => {
    const point = legendPoint(
      state,
      chart,
      x,
      top + (bottom - top) * fraction,
      aspect,
    );
    return { x: point.x * frame.width, y: point.y * frame.height };
  };
  const center = (fraction: number) => project(left + width / 2, fraction),
    firstPoint = center(0),
    lastPoint = center(1);
  chartCheck(
    lastPoint.y - firstPoint.y >= 8,
    `${chart.id}: focused numeric ramp spans enough pixels to distinguish its gradient`,
  );
  // Invert the independently projected centerline. Pixel centers and their
  // footprints remain meaningful when the 32 authored bands are subpixel.
  const fractionAtY = (pixelY: number) => {
    let low = 0,
      high = 1;
    for (let step = 0; step < 20; step++) {
      const middle = (low + high) / 2;
      if (center(middle).y < pixelY) low = middle;
      else high = middle;
    }
    return (low + high) / 2;
  };
  const samples: {
    x: number;
    y: number;
    fraction: number;
    rgb: number[];
    ranges: number[][];
    expected: number[];
  }[] = [];
  for (
    let y = Math.ceil(firstPoint.y + 0.5);
    y <= Math.floor(lastPoint.y - 1.5);
    y++
  ) {
    const fraction = fractionAtY(y + 0.5),
      point = center(fraction),
      x = Math.floor(point.x),
      leftPoint = project(left, fraction),
      rightPoint = project(left + width, fraction);
    chartCheck(
      x > 0 &&
        x < frame.width - 1 &&
        y > 0 &&
        y < frame.height - 1 &&
        x + 0.5 - leftPoint.x >= 0.75 &&
        rightPoint.x - x - 0.5 >= 0.75,
      `${chart.id}: gradient pixel lies inside the ramp rather than on an antialiased outer edge`,
    );
    const rgb = [
        ...frame.pixels.slice(
          (y * frame.width + x) * 4,
          (y * frame.width + x) * 4 + 3,
        ),
      ],
      low = Math.max(0, fractionAtY(y) - 1 / 31),
      high = Math.min(1, fractionAtY(y + 1) + 1 / 31),
      // Include the middle palette stop if the pixel footprint crosses it.
      colors = [low, high, ...(low < 0.5 && high > 0.5 ? [0.5] : [])].map(
        (value) => srgbColor(expectedHeightColor(4 * (1 - value))),
      ),
      ranges = [0, 1, 2].map((channel) => [
        Math.min(...colors.map((color) => color[channel]!)),
        Math.max(...colors.map((color) => color[channel]!)),
      ]),
      expected = srgbColor(expectedHeightColor(4 * (1 - fraction)));
    chartCheck(
      rgb.every(
        (value, channel) =>
          value >= ranges[channel]![0]! - 6 &&
          value <= ranges[channel]![1]! + 6,
      ),
      `${chart.id}: interior gradient pixel matches independently filtered palette coverage (pixel ${x},${y}, RGB ${rgb}, ranges ${JSON.stringify(ranges)})`,
    );
    samples.push({ x, y, fraction, rgb, ranges, expected });
  }
  chartCheck(
    samples.length >= 6 &&
      samples[0]!.fraction < 0.3 &&
      samples.at(-1)!.fraction > 0.7,
    `${chart.id}: gradient samples cover distinct high, middle and low scale colors`,
  );
  const greenError =
    samples.reduce(
      (total, sample) => total + sample.rgb[1]! - sample.expected[1]!,
      0,
    ) / samples.length;
  chartCheck(
    Math.abs(greenError) <= 6,
    `${chart.id}: opaque ramp preserves average palette brightness through minification (green error ${greenError})`,
  );
  chartCheck(
    samples.every(
      (sample, index) =>
        index === 0 || sample.rgb[1]! <= samples[index - 1]!.rgb[1]! + 3,
    ) &&
      samples[0]!.rgb[1]! > samples.at(-1)!.rgb[1]! + 40 &&
      samples[0]!.rgb[0]! > samples.at(-1)!.rgb[0]! + 80,
    `${chart.id}: visible gradient preserves high-to-low numeric color order`,
  );
  return { firstPoint, lastPoint, samples, greenError };
}

export function assertChartLegendState(state: GalleryChartsState) {
  for (const chart of state.charts) {
    const legend = chart.legend,
      style = legendFields(chart, legend.id, "scale_x"),
      height = Number(style.clip_max_y),
      width = Number(style.clip_max_x);
    chartCheck(
      width === 180 && height > 16 && legend.unitsPerMetre === 60,
      `${chart.id}: legend has explicit readable Canvas dimensions`,
    );
    if (chart.surface) {
      chartCheck(
        legend.world.id === chart.world.id &&
          legend.anchor === chart.anchor &&
          style.x === 624 &&
          style.y === (360 - height) / 2 &&
          legend.surface === "CylinderSurface",
        `${chart.id}: legend stays outside the right edge and centered on the existing curved Canvas`,
      );
    } else {
      const anchor = state.world.entities.find(
          (entity) => entity.id === legend.anchor,
        ),
        transform = anchor?.components.find(
          (component) => "qx" in component.fields,
        )?.fields,
        surface = anchor?.components.find(
          (component) => component.component === 25,
        )?.fields;
      chartCheck(
        anchor?.link.parent === chart.entity &&
          transform &&
          surface &&
          Math.abs(Number(transform.x) - 11.9) < 0.00001 &&
          Number(transform.y) === Number(chart.frame.height) / 2 &&
          Number(transform.z) === Number(chart.frame.depth) &&
          legend.center[2] === Number(chart.frame.depth) &&
          Number(transform.qw) === 1 &&
          [transform.qx, transform.qy, transform.qz].every(
            (value) => Number(value) === 0,
          ) &&
          Number(surface.width) === 3 &&
          Math.abs(Number(surface.height) - height / 60) < 0.00001 &&
          legend.world.id !== state.worldReference.id &&
          style.x === 0 &&
          style.y === 0 &&
          legend.surface === "FlatSurface",
        `${chart.id}: centered flat legend sits beside the front XY face and inherits the rotated chart Transform`,
      );
      chartCheck(
        legend.inspection.canvas?.evaluated?.extent[0] === 180 &&
          legend.inspection.canvas.evaluated.extent[1] === height,
        `${chart.id}: the real flat Surface presents its complete legend Canvas`,
      );
    }
    chartCheck(
      legendFields(chart, `${legend.id}/title`, "text").text === legend.title,
      `${chart.id}: legend title is real Canvas text`,
    );
    if (legend.entries) {
      for (const [index, entry] of legend.entries.entries()) {
        chartCheck(
          entry.color.every(
            (value, channel) =>
              Math.abs(value - categoryColors[index % 4]![channel]!) < 0.00001,
          ) &&
            legendFields(
              chart,
              `${legend.id}/entry/${encodeURIComponent(entry.id)}/label`,
              "text",
            ).text === entry.label,
          `${chart.id}: stable categories label the independently expected palette`,
        );
      }
    } else {
      chartCheck(
        legend.scale?.min === 0 &&
          legend.scale.max === 4 &&
          legendFields(chart, `${legend.id}/min`, "text").text === "0.0 M" &&
          legendFields(chart, `${legend.id}/max`, "text").text === "4.0 M",
        `${chart.id}: height legend labels its fixed numeric endpoint range`,
      );
    }
    const source = legendFields(chart, `${legend.id}/title`, "text").source;
    chartCheck(
      legend.inspection.resources.some(
        (resource) =>
          resource.source === source &&
          resource.status === "loaded" &&
          resource.representation.decoded,
      ),
      `${chart.id}: actual legend font is decoded across the World boundary`,
    );
  }
}

export function assertHeightBindingColors(state: GalleryChartsState) {
  const chart = state.charts.find((chart) => chart.id === "height-surface")!;
  const heightColumn = chart.binding.columns.findIndex(
      (column) => column.name === "y",
    ),
    colorColumn = chart.binding.columns.findIndex(
      (column) => column.name === "color",
    );
  for (const row of chart.binding.rows) {
    const height = row.values[heightColumn],
      color = row.values[colorColumn];
    if (!height?.valid) {
      chartCheck(
        color?.valid && color.value.kind === "vec4",
        "An intentional height gap retains its independent source color",
      );
      continue;
    }
    chartCheck(
      height?.valid &&
        height.value.kind === "f32" &&
        color?.valid &&
        color.value.kind === "vec4",
      "Height samples retain actual numeric values and source colors",
    );
    chartCheck(
      color.value.value.every(
        (value, channel) =>
          Math.abs(
            value - expectedHeightColor(height.value.value as number)[channel]!,
          ) < 0.00001,
      ),
      "Height binding color independently matches its numeric scale, including source edits and live arrivals",
    );
  }
}

export function assertChartLegendPixels(
  frame: ChartImage,
  state: GalleryChartsState,
  id: string,
) {
  const chart = state.charts.find((chart) => chart.id === id)!;
  const legend = chart.legend,
    root = legendFields(chart, legend.id, "scale_x"),
    aspect = frame.width / frame.height;
  const probes = (legend.entries ?? []).map((entry) => {
    const probe = {
      id: `${legend.id}/entry/${encodeURIComponent(entry.id)}/swatch`,
      color: entry.color,
    };
    const style = legendFields(chart, probe.id, "scale_x"),
      box = legendFields(chart, probe.id, "width");
    return probeChartColor(
      frame,
      legendPoint(
        state,
        chart,
        Number(root.x) + Number(style.x) + Number(box.width) / 2,
        Number(root.y) + Number(style.y) + Number(box.height) / 2,
        aspect,
      ),
      probe.color,
      probe.id,
    );
  });
  const ramp = legend.scale
    ? assertNumericLegendPixels(frame, state, chart)
    : null;
  // Check the complete panel, including its title lane, rather than only a
  // central mark. This also exercises the gallery's deliberately narrow canvas.
  for (const [x, y] of [
    [Number(root.x), Number(root.y)],
    [
      Number(root.x) + Number(root.clip_max_x),
      Number(root.y) + Number(root.clip_max_y),
    ],
  ]) {
    const point = legendPoint(state, chart, x!, y!, aspect);
    chartCheck(
      point.x > 0.01 && point.x < 0.99 && point.y > 0.01 && point.y < 0.99,
      `${id}: focus retains the whole legend inside the actual viewport`,
    );
  }
  if (id === "bars") {
    const xColumn = chart.binding.columns.findIndex(
        (column) => column.name === "x",
      ),
      yColumn = chart.binding.columns.findIndex(
        (column) => column.name === "y",
      );
    const heights = chart.binding.rows.flatMap((row) => {
      const x = row.values[xColumn],
        y = row.values[yColumn];
      return x?.valid &&
        x.value.kind === "f32" &&
        x.value.value === 2 &&
        y?.valid &&
        y.value.kind === "f32"
        ? [y.value.value]
        : [];
    });
    chartCheck(heights.length > 0, "Category two retains an actual bar sample");
    probes.push(
      probeChartColor(
        frame,
        baselineBarPointer(state, aspect, 2, Math.min(...heights) / 2),
        categoryColors[1],
        "bars source row two",
      ),
    );
  }
  if (id === "grid-bars")
    probes.push(
      probeChartColor(
        frame,
        baselineGridPointer(state, aspect),
        categoryColors[1],
        "grid source row six",
      ),
    );
  if (id === "single-row")
    probes.push(
      probeChartColor(
        frame,
        baselineSingleRowPointer(state, aspect),
        categoryColors[1],
        "single-row source row two",
      ),
    );
  if (id === "height-surface") {
    const value = (
      row: GalleryChartsState["charts"][number]["binding"]["rows"][number],
      column: string,
    ) => {
      const cell =
        row.values[
          chart.binding.columns.findIndex((item) => item.name === column)
        ];
      chartCheck(
        cell?.valid && cell.value.kind === "f32",
        `Height ${column} is a valid source value`,
      );
      return cell.value.value;
    };
    const heightColumn = chart.binding.columns.findIndex(
      (column) => column.name === "y",
    );
    const row = chart.binding.rows
      .filter((row) => row.values[heightColumn]?.valid)
      .sort((a, b) => value(b, "y") - value(a, "y"))[0]!;
    const local = ["x", "y", "z"].map(
      (axis, index) =>
        ((value(row, axis) - chart.frame[`min_${axis}`]!) /
          (chart.frame[`max_${axis}`]! - chart.frame[`min_${axis}`]!)) *
        chart.frame[["width", "height", "depth"][index]!]!,
    );
    probes.push(
      probeChartColor(
        frame,
        projectChartPoint(
          state,
          placePoint(chartTransform(state, chart), local),
          aspect,
        ),
        expectedHeightColor(value(row, "y")),
        "height surface peak shares its numeric legend color",
      ),
    );
  }
  return { id, probes, ramp };
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

export function assertChartImageStable(
  before: ChartImage,
  after: ChartImage,
  label: string,
) {
  chartCheck(
    before.width === after.width && before.height === after.height,
    `${label}: completed frame dimensions changed`,
  );
  chartCheck(
    before.pixels.every(
      (pixel, index) => Math.abs(pixel - after.pixels[index]!) <= 1,
    ),
    `${label}: stationary chart data retains completed pixels`,
  );
}

export function assertNoChartAnimation(state: GalleryChartsState) {
  chartCheck(
    state.charts.every(
      (chart) =>
        !chart.inspection.controllers?.some((controller) =>
          controller.description.drivers.some(
            (driver) => driver.target === chart.entity,
          ),
        ),
    ),
    "Stationary samples register no data animation controllers",
  );
}

export function assertBoundedChartInput(state: GalleryChartsState) {
  for (const [name, lane] of Object.entries(state.input)) {
    chartCheck(
      lane.maxInFlight <= 1 && lane.maxPending <= 1,
      `${name}: input retains at most one active and one pending operation`,
    );
    chartCheck(
      lane.inFlight === 0 && lane.pending === 0,
      `${name}: input settles without queued work`,
    );
  }
}

function bindingSamples(state: GalleryChartsState) {
  return JSON.stringify(
    state.charts.map((chart) => [chart.id, chart.binding.rows]),
    (_key, value) => (typeof value === "bigint" ? String(value) : value),
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
    state.ring.radius === 28 &&
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
    if (chart.surface) {
      const anchor = state.world.entities.find(
        (item) => item.id === chart.anchor,
      );
      const providers = anchor?.components.filter((item) =>
        [25, 54, 55].includes(item.component),
      );
      const surface = providers?.[0]?.fields;
      chartCheck(
        providers?.length === 1 && providers[0]!.component === 54 && surface,
        `${chart.id}: actual anchor declares only CylinderSurface`,
      );
      chartCheck(
        chart.surface.component === "CylinderSurface" &&
          Math.abs(Number(surface.curvature) + 1 / state.ring.radius) < 1e-8 &&
          Math.abs(chart.surface.curvature - Number(surface.curvature)) <
            1e-8 &&
          Math.abs(Number(surface.width) - 13.4) < 0.00001 &&
          Number(surface.height) === 6 &&
          Number(surface.layer_spacing) === 0,
        `${chart.id}: actual cylindrical fields share the arrangement radius and physical extent`,
      );
      for (const u of [0, 0.25, 0.5, 0.75, 1]) {
        const x = (u - 0.5) * chart.surface.width;
        for (const y of [-3, 0, 3]) {
          const point = placePoint(fields, chartSurfaceLocalPoint(state, x, y));
          const radial = [
            point[0]! - state.ring.center[0]!,
            point[2]! - state.ring.center[2]!,
          ];
          const front = rotate(
            [-Math.sin(x / radius), 0, Math.cos(x / radius)],
            quaternion,
          );
          chartCheck(
            Math.abs(Math.hypot(...radial) - radius) < 1e-4 &&
              Math.abs(point[1]! - (state.ring.center[1]! + y)) < 1e-4,
            `${chart.id}: edge/interior samples lie on the common ring cylinder`,
          );
          chartCheck(
            -(front[0]! * radial[0]! + front[2]! * radial[1]!) / radius >
              0.99999,
            `${chart.id}: edge/interior normals face the ring center`,
          );
        }
      }
      chartCheck(
        Math.abs(chartSurfaceLocalPoint(state, 6.7, 0)[2]! - 0.797778) < 0.001,
        `${chart.id}: panel edge sag uses radius28 rather than a visually exaggerated radius`,
      );
    }
    // A 13.4 × 10 metre footprint encloses every chart and its legend. Compare
    // oriented rectangles: bounding circles discard the inward-facing layout.
    const footprint = [
      [-6.7, -5],
      [6.7, -5],
      [6.7, 5],
      [-6.7, 5],
    ].map(([x, z]) => {
      const point = placePoint(fields, [
        chart.localCenter[0]! + x!,
        0,
        chart.localCenter[2]! + z!,
      ]);
      return [point[0]!, point[2]!] as const;
    });
    return {
      id: chart.id,
      center,
      angle: (Math.atan2(delta[2]!, delta[0]!) + 2 * Math.PI) % (2 * Math.PI),
      footprint,
    };
  });
  const ordered = [...exhibits].sort((a, b) => a.angle - b.angle);
  const adjacentDistances: number[] = [];
  let minimumClearance = Number.POSITIVE_INFINITY;
  let closest: readonly string[] = [];
  for (let i = 0; i < ordered.length; i++) {
    const next = ordered[(i + 1) % ordered.length]!;
    const gap =
      (ordered[(i + 1) % ordered.length]!.angle -
        ordered[i]!.angle +
        2 * Math.PI) %
      (2 * Math.PI);
    chartCheck(
      Math.abs(gap - (2 * Math.PI) / ordered.length) < 0.001,
      "Actual exhibits have equal angular spacing around the ring",
    );
    const adjacentDistance = Math.hypot(
      ...ordered[i]!.center.map((value, axis) => value - next.center[axis]!),
    );
    adjacentDistances.push(adjacentDistance);
    chartCheck(
      Math.abs(adjacentDistance - 2 * 28 * Math.sin(Math.PI / 10)) < 0.001,
      "The condensed ring retains the expected adjacent exhibit distance",
    );
    for (let j = i + 1; j < ordered.length; j++) {
      const a = ordered[i]!,
        b = ordered[j]!;
      const clearance = footprintClearance(a.footprint, b.footprint);
      chartCheck(
        clearance > 1,
        `${a.id}/${b.id}: chart volumes have clear space between them`,
      );
      if (clearance < minimumClearance) {
        minimumClearance = clearance;
        closest = [a.id, b.id];
      }
    }
  }
  return {
    radius: state.ring.radius,
    adjacentDistances,
    minimumClearance,
    closest,
  };
}

function footprintClearance(
  a: readonly (readonly number[])[],
  b: readonly (readonly number[])[],
) {
  const edges = (polygon: readonly (readonly number[])[]) =>
    polygon.map(
      (point, index) =>
        [point, polygon[(index + 1) % polygon.length]!] as const,
    );
  const separated = [...edges(a), ...edges(b)].some(([from, to]) => {
    const axis = [to[1]! - from[1]!, from[0]! - to[0]!];
    const projected = (polygon: readonly (readonly number[])[]) =>
      polygon.map((point) => point[0]! * axis[0]! + point[1]! * axis[1]!);
    const pa = projected(a),
      pb = projected(b);
    return (
      Math.max(...pa) < Math.min(...pb) || Math.max(...pb) < Math.min(...pa)
    );
  });
  if (!separated) return 0;
  const distance = (
    point: readonly number[],
    from: readonly number[],
    to: readonly number[],
  ) => {
    const dx = to[0]! - from[0]!,
      dy = to[1]! - from[1]!;
    const t = Math.max(
      0,
      Math.min(
        1,
        ((point[0]! - from[0]!) * dx + (point[1]! - from[1]!) * dy) /
          (dx * dx + dy * dy),
      ),
    );
    return Math.hypot(
      point[0]! - from[0]! - t * dx,
      point[1]! - from[1]! - t * dy,
    );
  };
  return Math.min(
    ...a.flatMap((point) =>
      edges(b).map(([from, to]) => distance(point, from, to)),
    ),
    ...b.flatMap((point) =>
      edges(a).map(([from, to]) => distance(point, from, to)),
    ),
  );
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
  const initial = await driver.inspect();
  assertNoChartAnimation(initial);
  assertChartLegendState(initial);
  assertHeightBindingColors(initial);
  const terrain = initial.charts.find(
      (chart) => chart.id === "height-surface",
    )!,
    terrainY = terrain.binding.columns.findIndex(
      (column) => column.name === "y",
    );
  chartCheck(
    terrain.binding.rows.filter((row) => !row.values[terrainY]?.valid)
      .length === 1,
    "The fixed surface retains its intentional missing-height sample",
  );
  await driver.record("charts-initial", initial);
  const center = await driver.capture("ring-center");
  assertChartImage(center, "ring center");
  const stationary = await waitForCharts(
    driver,
    (state) => state.world.time >= initial.world.time + 0.4,
    "fixed-default-stationary",
  );
  assertNoChartAnimation(stationary);
  chartCheck(
    bindingSamples(initial) === bindingSamples(stationary),
    "Default fixed samples remain unchanged as the Host clock advances",
  );
  assertChartImageStable(
    center,
    await driver.capture("ring-center-stationary"),
    "Default fixed samples",
  );
  await driver.record("charts-default-stationary", stationary);
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
    "One scene contains all ten Canvas and volumetric chart families",
  );
  await driver.record("charts-ring-separation", assertRingLayout(initial));
  assertCenterCamera(initial);
  const canvas = initial.charts.filter((chart) =>
    chart.component.endsWith("2d"),
  );
  const spatial = initial.charts.filter((chart) =>
    chart.component.endsWith("3d"),
  );
  chartCheck(
    canvas.length === 5 && spatial.length === 5,
    "Five Canvas plots and five spatial plots share the camera scene",
  );
  chartCheck(
    new Set(canvas.map((chart) => String(chart.world.id))).size === 5 &&
      canvas.every((chart) => chart.world.id !== initial.worldReference.id),
    "Each Canvas chart owns one distinct Canvas child World",
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
  const singleRow = initial.charts.find((chart) => chart.id === "single-row")!;
  const barDepth = singleRow.inspection.entities
    .find((entity) => entity.id === singleRow.entity)
    ?.components.find((component) => "bar_depth" in component.fields)
    ?.fields.bar_depth;
  chartCheck(
    singleRow.frame.depth === 2 &&
      singleRow.frame.min_z === -1 &&
      singleRow.frame.max_z === 1 &&
      Number(barDepth) === 1.5,
    "The single-row frame is two metres deep and encloses one centred 1.5 metre bar row with 0.25 metre margins",
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
    attachments.length === 10 &&
      attachments.every(
        ({ fields }) => Number(fields.mode) === 1 && fields.output === null,
      ),
    "SurfaceCanvas attachments present five curved charts and five spatial legends",
  );
  for (const chart of canvas) {
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
        chart.inspection.canvas.evaluated?.extent[0] === 804 &&
        chart.inspection.canvas.evaluated.extent[1] === 360,
      `${chart.id}: Surface presentation supplies the Canvas extent`,
    );
  }
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
  const afterManual = await waitForCharts(
    driver,
    (state) => state.world.time >= manual.world.time + 0.4,
    "manual-focus-cancel-stationary",
  );
  chartCheck(
    afterManual.focus === null &&
      JSON.stringify(afterManual.camera.transform) ===
        JSON.stringify(manual.camera.transform) &&
      !afterManual.world.controllers?.some(
        (controller) => controller.id === interrupted.focus?.controller,
      ),
    "Manual interruption leaves no delayed camera motion or focus controller",
  );
  assertBoundedChartInput(afterManual);
  await driver.record("focus-manual-cancellation", { manual, afterManual });

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
    await driver.record(
      `charts-focus-${id}-legend`,
      assertChartLegendPixels(focused, current, id),
    );
  }
  await focusChart(driver, "bars");
  await driver.action("navigate", { kind: "zoom", amount: 0.3 });
  await driver.action("navigate", { kind: "rotate", yaw: 0.4, pitch: 0.16 });
  const curvedView = await driver.inspect();
  const curvedFrame = await driver.capture("charts-inward-oblique");
  assertFocusedChartImage(curvedFrame, "inward oblique bars");
  await driver.record(
    "charts-inward-oblique-probes",
    assertCurvedChartPixels(curvedFrame, curvedView, "bars"),
  );
  await focusChart(driver, "bars");
  const before = await driver.capture("charts-fixed-original");
  await driver.action("changeSamples", true);
  const after = await driver.capture("charts-fixed-edited");
  assertChartImageChanged(before, after, "Fixed source edit");
  const changed = await driver.inspect();
  assertNoChartAnimation(changed);
  assertHeightBindingColors(changed);
  for (const chart of changed.charts) {
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
      `${chart.id}: explicit source edits change actual evaluated chart data`,
    );
  }
  const later = await waitForCharts(
    driver,
    (state) => state.world.time > changed.world.time + 0.4,
    "edited-fixed-stationary",
  );
  chartCheck(
    bindingSamples(changed) === bindingSamples(later),
    "Edited fixed samples remain unchanged while the Host clock advances",
  );
  assertChartImageStable(
    after,
    await driver.capture("charts-fixed-edited-stationary"),
    "Edited fixed samples",
  );
  await driver.record("charts-fixed-source-edit", { changed, later });
  await driver.action("changeSamples", false);
  await focusChart(driver, "grid-bars");
  const gridBefore = await driver.capture("charts-grid-fixed-original");
  await driver.action("changeSamples", true);
  const gridAfter = await driver.capture("charts-grid-fixed-edited");
  assertChartImageChanged(gridBefore, gridAfter, "Spatial fixed source edit");
  await driver.action("changeSamples", false);
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
export function baselineBarPointer(
  state: GalleryChartsState,
  aspect: number,
  sourceX = 2,
  sourceY = 32.5,
) {
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
    ((sourceX - Number(frame.min_x)) /
      (Number(frame.max_x) - Number(frame.min_x))) *
      (width - Number(frame.padding_left) - Number(frame.padding_right));
  const y =
    height -
    Number(frame.padding_bottom) -
    (sourceY / (Number(frame.max_y) - Number(frame.min_y))) *
      (height - Number(frame.padding_top) - Number(frame.padding_bottom));
  const density = Number(chart.surface?.unitsPerMetre);
  const point = placePoint(
    object,
    chartSurfaceLocalPoint(
      state,
      x / density - chart.surface!.width / 2,
      chart.surface!.height / 2 - y / density,
    ),
  );
  assertChartRay(
    state,
    chart,
    point,
    x / density - chart.surface!.width / 2,
    chart.surface!.height / 2 - y / density,
  );
  return projectChartPoint(state, point, aspect);
}

/** Independent ring-cylinder geometry, derived from the arrangement rather than runtime sampling. */
export function chartSurfaceLocalPoint(
  state: GalleryChartsState,
  x: number,
  y: number,
) {
  const radius = state.ring.radius;
  return [
    radius * Math.sin(x / radius),
    y,
    radius * (1 - Math.cos(x / radius)),
  ];
}

function assertChartRay(
  state: GalleryChartsState,
  chart: GalleryChartsState["charts"][number],
  point: readonly number[],
  x: number,
  y: number,
) {
  const origin = [
    state.camera.transform.x,
    state.camera.transform.y,
    state.camera.transform.z,
  ];
  const direction = point.map((v, i) => v - origin[i]!);
  const ox = origin[0]! - state.ring.center[0]!,
    oz = origin[2]! - state.ring.center[2]!;
  const a = direction[0]! ** 2 + direction[2]! ** 2;
  const b = 2 * (ox * direction[0]! + oz * direction[2]!);
  const c = ox ** 2 + oz ** 2 - state.ring.radius ** 2;
  const discriminant = b * b - 4 * a * c;
  chartCheck(
    discriminant >= 0,
    "Projected pointer ray intersects the ring cylinder",
  );
  const roots = [
    (-b - Math.sqrt(discriminant)) / (2 * a),
    (-b + Math.sqrt(discriminant)) / (2 * a),
  ];
  const t = roots.find((value) => value > 0 && Math.abs(value - 1) < 1e-4);
  chartCheck(
    t !== undefined,
    "Independent ray recovers the authored chart point",
  );
  const fields = chartTransform(state, chart);
  const local = rotate(
    origin.map(
      (v, i) => v + t * direction[i]! - Number(fields[["x", "y", "z"][i]!]),
    ),
    [
      -Number(fields.qx),
      -Number(fields.qy),
      -Number(fields.qz),
      Number(fields.qw),
    ],
  );
  chartCheck(
    Math.abs(
      state.ring.radius * Math.atan2(local[0]!, state.ring.radius - local[2]!) -
        x,
    ) < 1e-4 && Math.abs(local[1]! - y) < 1e-4,
    "Independent ray inversion returns the same arc-length chart coordinates",
  );
}

/** Compare completed pixels at independent curved-edge and data locations in an oblique view. */
export function assertCurvedChartPixels(
  frame: ChartImage,
  state: GalleryChartsState,
  id: string,
) {
  const chart = state.charts.find((chart) => chart.id === id)!;
  chartCheck(chart.surface, "Curved pixel probe has a Canvas chart");
  const object = chartTransform(state, chart),
    aspect = frame.width / frame.height;
  const plotFrame = chart.inspection.entities
    .find((entity) => entity.id === chart.entity)
    ?.components.find(
      (component) => "padding_bottom" in component.fields,
    )?.fields;
  chartCheck(plotFrame, "Curved pixel probe has the authored Canvas frame");
  const canvasHeight = Number(plotFrame.height),
    fontSize = Number(plotFrame.font_size),
    tickBottom = canvasHeight - Number(plotFrame.padding_bottom) + 8 + fontSize,
    titleTop = canvasHeight - fontSize - 2;
  chartCheck(tickBottom < titleTop, "Canvas fixture has a blank bottom margin");
  // The fixture's numeric labels end above its X title. Sample the middle of
  // this blank margin, keeping every probe away from grid, bars and glyph ink.
  const canvasY = (tickBottom + titleTop) / 2,
    localY = chart.surface.height / 2 - canvasY / chart.surface.unitsPerMetre;
  const probes = [0.05, 0.25, 0.75, 0.95].map((u) => {
    const x = (u - 0.5) * chart.surface!.width;
    const local = chartSurfaceLocalPoint(state, x, localY);
    const projected = projectChartPoint(
      state,
      placePoint(object, local),
      aspect,
    );
    const flat = projectChartPoint(
      state,
      placePoint(object, [x, local[1]!, 0]),
      aspect,
    );
    const px = Math.round(projected.x * frame.width),
      py = Math.round(projected.y * frame.height);
    chartCheck(
      px > 1 && px < frame.width - 2 && py > 1 && py < frame.height - 2,
      "Curved edge probe is inside completed frame",
    );
    const rgb = [
      ...frame.pixels.slice(
        (py * frame.width + px) * 4,
        (py * frame.width + px) * 4 + 3,
      ),
    ];
    chartCheck(
      rgb.every((v, i) => Math.abs(v - [33, 44, 56][i]!) < 15),
      `${id}: inward-curved panel background at independent edge probe ${u}: ${rgb}`,
    );
    return {
      u,
      canvasY,
      projected,
      flat,
      rgb,
      displacement: Math.hypot(
        (projected.x - flat.x) * frame.width,
        (projected.y - flat.y) * frame.height,
      ),
    };
  });
  chartCheck(
    Math.max(...probes.map((p) => p.displacement)) > 1,
    "Actual-radius curvature shifts oblique edge probes by observable pixels",
  );
  const bar = baselineBarPointer(state, aspect);
  const offset =
    (Math.round(bar.y * frame.height) * frame.width +
      Math.round(bar.x * frame.width)) *
    4;
  const rgb = [...frame.pixels.slice(offset, offset + 3)];
  chartCheck(
    rgb.every(
      (value, channel) =>
        Math.abs(value - srgbColor(categoryColors[1])[channel]!) < 20,
    ),
    `${id}: independently projected source-row interior matches its orange legend category: ${rgb}`,
  );
  const silhouette = curvedSilhouetteProbe(frame, state, chart);
  return { probes, bar: { point: bar, rgb }, silhouette };
}

/** A silhouette pixel must belong to exactly one of the cylindrical and tangent-plane extents. */
function curvedSilhouetteProbe(
  frame: ChartImage,
  state: GalleryChartsState,
  chart: GalleryChartsState["charts"][number],
) {
  const fields = chartTransform(state, chart),
    surface = chart.surface!;
  const inverse = [
    -Number(fields.qx),
    -Number(fields.qy),
    -Number(fields.qz),
    Number(fields.qw),
  ];
  const origin = rotate(
    [
      state.camera.transform.x - Number(fields.x),
      state.camera.transform.y - Number(fields.y),
      state.camera.transform.z - Number(fields.z),
    ],
    inverse,
  );
  const cameraRotation = [
    state.camera.transform.qx,
    state.camera.transform.qy,
    state.camera.transform.qz,
    state.camera.transform.qw,
  ];
  const radius = state.ring.radius,
    aspect = frame.width / frame.height;
  const tangent = Math.tan(Number(state.camera.fields.fov_y) / 2);
  chartCheck(
    Number(state.camera.fields.projection) === 0 &&
      [fields.sx, fields.sy, fields.sz].every((v) => Number(v) === 1),
    "Silhouette oracle uses the actual perspective camera and unscaled exhibit",
  );
  const candidates = [];
  for (const edge of [-1, 1]) {
    const x = (edge * surface.width) / 2;
    for (const y of [-1, 0, 1, 2.5]) {
      const curved = projectChartPoint(
        state,
        placePoint(fields, chartSurfaceLocalPoint(state, x, y)),
        aspect,
      );
      const planar = projectChartPoint(
        state,
        placePoint(fields, [x, y, 0]),
        aspect,
      );
      for (const fraction of [0.25, 0.5, 0.75]) {
        const px = Math.round(
          (curved.x + fraction * (planar.x - curved.x)) * frame.width,
        );
        const py = Math.round(
          (curved.y + fraction * (planar.y - curved.y)) * frame.height,
        );
        if (px < 0 || px >= frame.width || py < 0 || py >= frame.height)
          continue;
        // Unproject the exact sampled pixel, then independently intersect both possible providers.
        const direction = rotate(
          rotate(
            [
              (2 * ((px + 0.5) / frame.width) - 1) * tangent * aspect,
              (1 - 2 * ((py + 0.5) / frame.height)) * tangent,
              -1,
            ],
            cameraRotation,
          ),
          inverse,
        );
        const tPlane = -origin[2]! / direction[2]!;
        const planePoint = origin.map((v, i) => v + tPlane * direction[i]!);
        const a = direction[0]! ** 2 + direction[2]! ** 2;
        const b =
          2 *
          (origin[0]! * direction[0]! + (origin[2]! - radius) * direction[2]!);
        const c = origin[0]! ** 2 + (origin[2]! - radius) ** 2 - radius ** 2;
        const d = b * b - 4 * a * c;
        if (d < 0) continue;
        const hits = [
          (-b - Math.sqrt(d)) / (2 * a),
          (-b + Math.sqrt(d)) / (2 * a),
        ]
          .filter((t) => t > 0)
          .map((t) => {
            const point = origin.map((v, i) => v + t * direction[i]!);
            return [
              radius * Math.atan2(point[0]!, radius - point[2]!),
              point[1]!,
            ];
          });
        const margin = 0.035;
        const inside = (point: readonly number[]) =>
          Math.abs(point[0]!) < surface.width / 2 - margin &&
          Math.abs(point[1]!) < surface.height / 2 - margin;
        const outside = (point: readonly number[]) =>
          Math.abs(point[0]!) > surface.width / 2 + margin ||
          Math.abs(point[1]!) > surface.height / 2 + margin;
        const curvedInside = hits.some(inside),
          planeInside = tPlane > 0 && inside(planePoint);
        if (
          !(curvedInside && outside(planePoint)) &&
          !(planeInside && hits.every(outside))
        )
          continue;
        const rgb = [
          ...frame.pixels.slice(
            (py * frame.width + px) * 4,
            (py * frame.width + px) * 4 + 3,
          ),
        ];
        const panel = rgb.every((v, i) => Math.abs(v - [33, 44, 56][i]!) < 15);
        // RenderService's linear clear colour converts to this independent sRGB reference.
        const background = rgb.every(
          (v, i) => Math.abs(v - [10, 14, 20][i]!) < 4,
        );
        candidates.push({
          px,
          py,
          curvedInside,
          planeInside,
          planePoint,
          hits,
          rgb,
        });
        chartCheck(
          curvedInside ? panel : background,
          `Cylindrical silhouette rejects planar presentation at ${px},${py}: ${rgb}`,
        );
      }
    }
  }
  chartCheck(
    candidates.length > 0,
    "Actual-radius curved silhouette has a pixel distinguishable from the tangent plane",
  );
  return candidates;
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
    placePoint(chartTransform(state, chart), [3.75, 2.5, 1]),
    aspect,
  );
}

export async function exerciseChartRow(
  driver: GalleryChartsDriver,
  aspect: number,
) {
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
  await exerciseChartFeedbackEdits(driver, aspect);
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

export async function exerciseChartFeedbackEdits(
  driver: GalleryChartsDriver,
  aspect: number,
) {
  const state = await focusChart(driver, "bars");
  // Source row one is x=1,y=35; y=8.75 remains inside it after its value halves.
  const point = baselineBarPointer(state, aspect, 1, 8.75);
  await driver.action("hover", point);
  await driver.action("select", point);
  const selected = await waitForCharts(
    driver,
    (current) =>
      String(current.hover?.rowId) === "1" &&
      String(current.selection?.rowId) === "1",
    "fixed-row-one-feedback",
  );
  const number = (mark: ChartRow | null) => {
    const value = mark?.values[mark.columns.indexOf("y")];
    return value?.valid && value.value.kind === "f32" ? value.value.value : NaN;
  };
  chartCheck(
    number(selected.hover) === 35 && number(selected.selection) === 35,
    "Row feedback initially reflects raw source values",
  );
  for (const [changed, expected] of [
    [true, 17.5],
    [false, 35],
  ] as const) {
    await driver.action("changeSamples", changed);
    const refreshed = await waitForCharts(
      driver,
      (current) =>
        number(current.hover) === expected &&
        number(current.selection) === expected,
      `fixed-row-one-feedback-${changed ? "edited" : "restored"}`,
    );
    chartCheck(
      refreshed.hover?.rowId === selected.hover?.rowId &&
        refreshed.selection?.rowId === selected.selection?.rowId &&
        refreshed.selection?.entity === selected.selection?.entity &&
        refreshed.selection?.world.id === selected.selection?.world.id,
      "Retained feedback keeps its source identity while source values change",
    );
    assertNoChartAnimation(refreshed);
    await driver.record(
      `charts-feedback-${changed ? "edited" : "restored"}`,
      refreshed,
    );
  }
  await driver.action("clearSelection");
  await driver.action("hover", null);
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
