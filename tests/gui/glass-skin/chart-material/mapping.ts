/** One fixture mapping drives geometry, grids, labels and annotation anchors. */
export const VALUES = [24, 64, 42, 82, 55, 74] as const;
export const SHEET = { width: 740, height: 460 } as const;
export const EXTENT = {
  width: 1480,
  height: 920,
  devicePixelRatio: 2,
} as const;
export const PLOT = { x: 82, y: 84, width: 632, height: 308 } as const;
export const VIEW = { x: 20, y: 48, width: 700, height: 390 } as const;
export const FRAME = { width: 8, height: 3, depth: 6 } as const;
export const TARGET = [4, 1.35, 3] as const;
export const EYE = [12, 7.8, 13.5] as const;
export const ORTHO = 7.8;
export type V3 = readonly [number, number, number];
export type Point = readonly [number, number];
const dot = (a: V3, b: V3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const normalized = (v: V3): V3 => {
  const l = Math.hypot(...v);
  return [v[0] / l, v[1] / l, v[2] / l];
};
export const NORMAL = normalized([
  EYE[0] - TARGET[0],
  EYE[1] - TARGET[1],
  EYE[2] - TARGET[2],
]);
export const RIGHT = normalized([NORMAL[2], 0, -NORMAL[0]]);
// Cross(normal,right) is camera up.
export const CAMERA_UP: V3 = [
  NORMAL[1] * RIGHT[2],
  NORMAL[2] * RIGHT[0] - NORMAL[0] * RIGHT[2],
  -NORMAL[1] * RIGHT[0],
];
export const PIXELS_PER_UNIT = VIEW.height / ORTHO;
export function pose() {
  const yaw = Math.atan2(NORMAL[0], NORMAL[2]) / 2,
    pitch = -Math.asin(NORMAL[1]) / 2;
  return {
    x: EYE[0],
    y: EYE[1],
    z: EYE[2],
    qx: Math.cos(yaw) * Math.sin(pitch),
    qy: Math.sin(yaw) * Math.cos(pitch),
    qz: -Math.sin(yaw) * Math.sin(pitch),
    qw: Math.cos(yaw) * Math.cos(pitch),
  };
}
export function project(point: V3): Point {
  const d: V3 = [
    point[0] - TARGET[0],
    point[1] - TARGET[1],
    point[2] - TARGET[2],
  ];
  return [
    VIEW.x + VIEW.width / 2 + dot(d, RIGHT) * PIXELS_PER_UNIT,
    VIEW.y + VIEW.height / 2 - dot(d, CAMERA_UP) * PIXELS_PER_UNIT,
  ];
}
export function bar2d(i: number) {
  const width = 72,
    height = (VALUES[i]! / 100) * PLOT.height;
  return {
    x: PLOT.x + ((i + 0.5) * PLOT.width) / 6 - width / 2,
    y: PLOT.y + PLOT.height - height,
    width,
    height,
  };
}
export function bar3d(i: number) {
  const height = (VALUES[i % 6]! / 100) * FRAME.height;
  return {
    x: (((i % 4) + 0.5) * FRAME.width) / 4,
    y: height / 2,
    z: ((Math.floor(i / 4) + 0.5) * FRAME.depth) / 3,
    width: 1.05,
    height,
    depth: 1.2,
  };
}
export function corners3d(i: number): V3[] {
  const b = bar3d(i);
  return [-1, 1].flatMap((x) =>
    [-1, 1].flatMap((y) =>
      [-1, 1].map(
        (z) =>
          [
            b.x + (x * b.width) / 2,
            b.y + (y * b.height) / 2,
            b.z + (z * b.depth) / 2,
          ] as V3,
      ),
    ),
  );
}
function hull(points: Point[]): Point[] {
  const sorted = [...points].sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const cross = (a: Point, b: Point, c: Point) =>
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
  const half = (p: Point[]) => {
    const h: Point[] = [];
    for (const v of p) {
      while (h.length >= 2 && cross(h[h.length - 2]!, h[h.length - 1]!, v) <= 0)
        h.pop();
      h.push(v);
    }
    return h;
  };
  return [...half(sorted).slice(0, -1), ...half(sorted.reverse()).slice(0, -1)];
}
/** Camera-facing six-sided halo plane. Interior is discarded; opaque bars retain depth. */
export function halo3d(i: number) {
  const corners = corners3d(i),
    outline = hull(corners.map(project));
  const minX = Math.min(...outline.map((p) => p[0])) - 8,
    minY = Math.min(...outline.map((p) => p[1])) - 8;
  const width = Math.max(...outline.map((p) => p[0])) + 8 - minX,
    height = Math.max(...outline.map((p) => p[1])) + 8 - minY;
  const center = [minX + width / 2, minY + height / 2] as const,
    b = bar3d(i);
  const near =
    Math.max(
      ...corners.map((c) => dot([c[0] - b.x, c[1] - b.y, c[2] - b.z], NORMAL)),
    ) + 0.02;
  const screenCenter = project([b.x, b.y, b.z]);
  const dx = (center[0] - screenCenter[0]) / PIXELS_PER_UNIT,
    dy = -(center[1] - screenCenter[1]) / PIXELS_PER_UNIT;
  const position = [b.x, b.y, b.z].map(
    (v, k) => v + RIGHT[k]! * dx + CAMERA_UP[k]! * dy + NORMAL[k]! * near,
  );
  const edges = outline.map((p, j) => {
    const q = outline[(j + 1) % outline.length]!;
    return [p[0] - minX, p[1] - minY, q[0] - minX, q[1] - minY];
  });
  return {
    outline,
    minX,
    minY,
    width,
    height,
    edges,
    transform: {
      ...pose(),
      x: position[0]!,
      y: position[1]!,
      z: position[2]!,
      sx: width / PIXELS_PER_UNIT,
      sy: height / PIXELS_PER_UNIT,
      sz: 0.0001,
    },
  };
}
export const CALLOUT2 = {
  x: bar2d(1).x + 36 + 14,
  y: 126,
  width: 150,
  height: 34,
  cut: 9,
};
export function callout3d() {
  const b = bar3d(1),
    anchor = project([b.x, b.height, b.z]);
  return {
    anchor,
    box: {
      x: anchor[0] + 32,
      y: anchor[1] - 60,
      width: 150,
      height: 34,
      cut: 9,
    },
  };
}
