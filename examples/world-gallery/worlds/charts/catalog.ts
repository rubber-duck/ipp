import { curvedSurfaceFromRadius, type DatasetValue } from "@ipp/client";
import { plotColorScaleColor, type PlotLegendEntry } from "@ipp/react";
import {
  CHART_COLORS,
  CHART_HEIGHT_SCALE,
  chartCategoryColor,
  chartLegendEntries,
} from "./colors.js";

export type ChartPoint = readonly [number, number, number];
export interface ChartSpec {
  readonly id: string;
  readonly title: string;
  readonly component:
    | "PlotLine2d"
    | "PlotBars2d"
    | "PlotPie2d"
    | "PlotGridBars3d"
    | "PlotHeightSurface3d"
    | "PlotPoints3d"
    | "PlotPie3d";
  /** Authored entity origin, separate from the physical exhibit centre. */
  readonly position: ChartPoint;
  readonly center: ChartPoint;
  readonly localCenter: ChartPoint;
  readonly yaw: number;
  readonly rotation: readonly [number, number, number, number];
  readonly rows: readonly (readonly DatasetValue[])[];
  readonly frame: Readonly<Record<string, number | boolean | string>>;
  readonly style: Readonly<Record<string, number | boolean>>;
  readonly secondSeries?: boolean;
  readonly colorByRow?: boolean;
  readonly legendEntries?: readonly PlotLegendEntry[];
  readonly legendTitle: string;
}

export const CHART_SCHEMA = [
  { name: "x", kind: "f32" },
  { name: "y", kind: "f32" },
  { name: "z", kind: "f32" },
  { name: "y2", kind: "f32" },
  { name: "valid", kind: "f32" },
  { name: "radius", kind: "f32" },
  { name: "height", kind: "f32" },
  { name: "color", kind: "vec4" },
] as const;

export function chartData(
  x: number,
  y: number,
  z = 0,
  y2 = y,
  valid = 1,
  radius = 1,
  height = 0.5,
  color: readonly number[] = CHART_COLORS[0],
): DatasetValue[] {
  const values: DatasetValue[] = [x, y, z, y2, valid, radius, height].map(
    (value) => ({ kind: "f32", value }),
  );
  values.push({ kind: "vec4", value: [...color] });
  return values;
}
const sample = chartData;

const line = [
  sample(0, 10, 0, 20),
  sample(2, 45, 0, 35),
  sample(4, 65, 0, 60),
  sample(6, 0, 0, 0, 0),
  sample(8, 35, 0, 45),
  sample(10, 85, 0, 75),
];
const bars = [35, 75, 45, 25, 60, 85, 50, 30, 55, 70, 40, 20].map((value, i) =>
  sample(
    i % 4,
    value,
    Math.floor(i / 4),
    15 + ((i * 17) % 75),
    1,
    1,
    0.5,
    chartCategoryColor(i),
  ),
);
const pie = [40, 30, 20, 10].map((value, i) =>
  sample(
    0,
    value,
    0,
    value,
    1,
    [3, 2.4, 2.1, 2.7][i]!,
    [2.4, 1.5, 0.9, 1.8][i]!,
    chartCategoryColor(i),
  ),
);
const terrain: DatasetValue[][] = [];
for (let x = 0; x <= 16; x++)
  for (let z = 0; z <= 16; z++) {
    const px = (x * 10) / 16,
      pz = (z * 10) / 16;
    const height =
      0.3 +
      2.5 * Math.exp(-((px - 3) ** 2 + (pz - 4) ** 2) / 5) +
      2.8 * Math.exp(-((px - 7.5) ** 2 + (pz - 7.5) ** 2) / 4);
    terrain.push(
      sample(
        px,
        height,
        pz,
        0,
        x === 8 && z === 8 ? 0 : 1,
        1,
        0.5,
        plotColorScaleColor(CHART_HEIGHT_SCALE, height),
      ),
    );
  }

const flatFrame = {
  width: 600,
  height: 360,
  padding_left: 48,
  padding_top: 24,
  padding_right: 24,
  padding_bottom: 48,
  min_x: 0,
  max_x: 10,
  min_y: 0,
  max_y: 100,
  automatic_x: false,
  automatic_y: false,
  ticks: 5,
  font_size: 14,
  x_title: "TIME / S",
  y_title: "SIGNAL / %",
};
const spatialFrame = {
  width: 10,
  height: 5,
  depth: 10,
  min_x: -0.5,
  max_x: 3.5,
  min_y: 0,
  max_y: 100,
  min_z: -0.5,
  max_z: 2.5,
  automatic_x: false,
  automatic_y: false,
  automatic_z: false,
  adaptive_axes: false,
  ticks: 4,
  font_size: 0.27,
  x_title: "POSITION X",
  y_title: "HEIGHT Y",
  z_title: "POSITION Z",
};

export const CHART_RING = {
  center: [0, 6, 0] as ChartPoint,
  radius: 28,
} as const;

export const CHART_CANVAS_EXTENT = [804, 360] as const;
export const CHART_UNITS_PER_METRE = 60;
export const CHART_LEGEND_WIDTH = 180;
export const CHART_LEGEND_GAP = 24;

export const CHART_DISPLAY_WIDTH =
  (600 + CHART_LEGEND_GAP + CHART_LEGEND_WIDTH) / CHART_UNITS_PER_METRE;

/** Canvas chart widths are arc lengths on the same cylinder as the exhibit arrangement. */
export const CHART_SURFACE = curvedSurfaceFromRadius({
  width: CHART_DISPLAY_WIDTH,
  height: 6,
  radius: CHART_RING.radius,
  facing: "inside",
});

/** Rotate chart-local points about the exhibit's vertical axis. */
export function rotateChartPoint(
  point: readonly number[],
  yaw: number,
): ChartPoint {
  const c = Math.cos(yaw),
    s = Math.sin(yaw);
  return [
    c * point[0]! + s * point[2]!,
    point[1]!,
    -s * point[0]! + c * point[2]!,
  ];
}

const chartDefinitions: readonly Omit<
  ChartSpec,
  "position" | "center" | "localCenter" | "yaw" | "rotation"
>[] = [
  {
    id: "straight",
    title: "Straight line",
    component: "PlotLine2d",
    rows: line,
    frame: flatFrame,
    style: { interpolation: 0, line_width: 2.5, marker_size: 7 },
    legendTitle: "SIGNAL",
    legendEntries: chartLegendEntries(["Group A"]),
  },
  {
    id: "smooth",
    title: "Smooth lines",
    component: "PlotLine2d",
    rows: line,
    frame: flatFrame,
    style: { interpolation: 1, line_width: 2.5, marker_size: 6 },
    secondSeries: true,
    legendTitle: "SIGNALS",
    legendEntries: chartLegendEntries(["Group A", "Group B"]),
  },
  {
    id: "bars",
    title: "Bar chart",
    component: "PlotBars2d",
    rows: [35, 65, 50, 80].map((y, i) =>
      sample(i + 1, y, 0, y, 1, 1, 0.5, chartCategoryColor(i)),
    ),
    frame: {
      ...flatFrame,
      max_x: 5,
      x_title: "NODE / BIN",
      y_title: "OUTPUT / UNITS",
    },
    style: { gap: 0.28 },
    colorByRow: true,
    legendTitle: "NODES",
    legendEntries: chartLegendEntries(["Node 1", "Node 2", "Node 3", "Node 4"]),
  },
  {
    id: "bins",
    title: "Pre-binned input",
    component: "PlotBars2d",
    rows: [20, 45, 65, 35].map((y, i) =>
      sample(i + 1, y, 0, y, 1, 1, 0.5, chartCategoryColor(i)),
    ),
    frame: { ...flatFrame, max_x: 5, x_title: "BIN", y_title: "COUNT" },
    style: { gap: 0.25 },
    colorByRow: true,
    legendTitle: "BINS",
    legendEntries: chartLegendEntries(["Bin 1", "Bin 2", "Bin 3", "Bin 4"]),
  },
  {
    id: "pie",
    title: "Resource allocation",
    component: "PlotPie2d",
    rows: pie,
    frame: flatFrame,
    style: {},
    colorByRow: true,
    legendTitle: "RESOURCES",
    legendEntries: chartLegendEntries([
      "Compute",
      "Storage",
      "Network",
      "Reserve",
    ]),
  },
  {
    id: "grid-bars",
    title: "Grid bars",
    component: "PlotGridBars3d",
    rows: bars,
    frame: spatialFrame,
    style: { bar_width: 1.3, bar_depth: 1.5 },
    colorByRow: true,
    legendTitle: "COLUMNS",
    legendEntries: chartLegendEntries([
      "Column 1",
      "Column 2",
      "Column 3",
      "Column 4",
    ]),
  },
  {
    id: "single-row",
    title: "Single row",
    component: "PlotGridBars3d",
    rows: [30, 50, 35, 45].map((y, i) =>
      sample(i, y, 0, y, 1, 1, 0.5, chartCategoryColor(i)),
    ),
    frame: { ...spatialFrame, depth: 2, min_z: -1, max_z: 1, z_title: "ROW" },
    style: { bar_width: 1.3, bar_depth: 1.5 },
    colorByRow: true,
    legendTitle: "COLUMNS",
    legendEntries: chartLegendEntries([
      "Column 1",
      "Column 2",
      "Column 3",
      "Column 4",
    ]),
  },
  {
    id: "height-surface",
    title: "Height surface",
    component: "PlotHeightSurface3d",
    rows: terrain,
    frame: {
      ...spatialFrame,
      min_x: 0,
      max_x: 10,
      max_y: 4,
      min_z: 0,
      max_z: 10,
      y_title: "HEIGHT / M",
    },
    style: { wireframe: true, line_width: 0.018 },
    colorByRow: true,
    legendTitle: "HEIGHT / M",
  },
  {
    id: "point-plot",
    title: "Point groups",
    component: "PlotPoints3d",
    rows: bars,
    frame: spatialFrame,
    style: { marker_size: 0.23, marker_shape: 1 },
    secondSeries: true,
    legendTitle: "GROUPS",
    legendEntries: chartLegendEntries(["Group A", "Group B"]),
  },
  {
    id: "variable-pie",
    title: "Variable pie",
    component: "PlotPie3d",
    rows: pie,
    frame: spatialFrame,
    style: { start_angle: Math.PI / 2 },
    colorByRow: true,
    legendTitle: "RESOURCES",
    legendEntries: chartLegendEntries([
      "Compute",
      "Storage",
      "Network",
      "Reserve",
    ]),
  },
];

export const CHART_CATALOG: readonly ChartSpec[] = chartDefinitions.map(
  (spec, index) => {
    const angle = (index * Math.PI * 2) / chartDefinitions.length;
    const yaw = angle + Math.PI;
    const center: ChartPoint = [
      CHART_RING.center[0] + CHART_RING.radius * Math.sin(angle),
      CHART_RING.center[1],
      CHART_RING.center[2] + CHART_RING.radius * Math.cos(angle),
    ];
    const localCenter: ChartPoint = spec.component.endsWith("2d")
      ? [0, 0, 0]
      : [
          CHART_DISPLAY_WIDTH / 2,
          spec.component === "PlotPie3d" ? 1.5 : Number(spec.frame.height) / 2,
          Number(spec.frame.depth) / 2,
        ];
    const offset = rotateChartPoint(localCenter, yaw);
    const position: ChartPoint = [
      center[0] - offset[0],
      center[1] - offset[1],
      center[2] - offset[2],
    ];
    return {
      ...spec,
      center,
      localCenter,
      position,
      yaw,
      rotation: [0, Math.sin(yaw / 2), 0, Math.cos(yaw / 2)] as const,
    };
  },
);
