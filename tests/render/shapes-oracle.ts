/** Analytic image and geometry oracle independent of the runtime mesh generator. */
import type { FrameCapture } from "@ipp/client";
import {
  BACKGROUND_RGB,
  VIEWPORT,
  type ImageSummary,
} from "./image-assertions.js";
import type { ShapeDefinition } from "./shapes-fixture.js";

export type Vec3 = readonly [number, number, number];
export type Point = readonly [number, number];
export type BaseShape = "cube" | "sphere" | "pill" | "plane" | "cone";

const EYE: Vec3 = [3, 2, 5];
const BACK = normalize(EYE);
const RIGHT = normalize([BACK[2], 0, -BACK[0]]);
const UP = cross(BACK, RIGHT);
const FIELD_TANGENT = Math.tan(Math.PI / 8);
const PILL_RADIUS = 0.65;
const PILL_BODY_HALF_HEIGHT = 0.75;
const PLANE_HALF_SIZE = 1;
const PLANE_NORMAL_LENGTH = 1.25;
const PLANE_NORMAL_OFFSET = 0.1;
const PLANE_STROKE = 0.05;
export function foregroundMask(frame: FrameCapture): Uint8Array<ArrayBuffer> {
  const mask = new Uint8Array(frame.width * frame.height);
  for (let index = 0; index < mask.length; index += 1) {
    if (maximumDifference(pixelRgb(frame, index), BACKGROUND_RGB) > 8)
      mask[index] = 1;
  }
  return mask;
}

export function colorMask(
  frame: FrameCapture,
  colors: readonly Vec3[],
  tolerance: number,
): Uint8Array<ArrayBuffer> {
  const mask = new Uint8Array(frame.width * frame.height);
  for (let index = 0; index < mask.length; index += 1) {
    const rgb = pixelRgb(frame, index);
    if (colors.some((color) => maximumDifference(rgb, color) <= tolerance)) {
      mask[index] = 1;
    }
  }
  return mask;
}

export function oraclePixels(
  frame: FrameCapture,
  definition: ShapeDefinition,
  difference: boolean,
  rotationY: number,
): Uint8ClampedArray<ArrayBuffer> {
  const analytic = analyticMask(frame, definition.base, rotationY);
  const foreground = foregroundMask(frame);
  const contours = definition.outline
    ? projectedContours(definition.base, rotationY, definition.rings)
    : [];
  const arrow =
    definition.base === "plane" ? projectedNormal(rotationY) : undefined;
  const planeColors =
    definition.base === "plane" && !definition.outline
      ? colorMask(
          frame,
          [
            definition.material.map(linearToSrgb8) as unknown as Vec3,
            ...checkerColors(
              definition.material.map((factor) => factor * 0.25),
            ),
          ],
          4,
        )
      : undefined;
  const pixels = new Uint8ClampedArray(frame.width * frame.height * 4);
  for (let y = 0; y < frame.height; y += 1) {
    for (let x = 0; x < frame.width; x += 1) {
      const index = y * frame.width + x;
      const expected = definition.outline
        ? distanceToPolylines([x + 0.5, y + 0.5], contours) <= 2.5
        : analytic[index] === 1;
      const actual = planeColors
        ? planeColors[index] === 1
        : foreground[index] === 1;
      const offset = index * 4;
      const rgb = difference
        ? expected === actual
          ? [0, 0, 0]
          : expected
            ? [0, 180, 255]
            : [255, 80, 0]
        : expected
          ? arrow && distanceToPolylines([x + 0.5, y + 0.5], [arrow]) <= 3
            ? [255, 255, 255]
            : definition.base === "plane" && !definition.outline
              ? [137, 137, 137]
              : [238, 238, 238]
          : BACKGROUND_RGB;
      pixels[offset] = rgb[0];
      pixels[offset + 1] = rgb[1];
      pixels[offset + 2] = rgb[2];
      pixels[offset + 3] = 255;
    }
  }
  return pixels;
}

export function analyticMask(
  frame: FrameCapture,
  shape: BaseShape,
  rotationY: number,
): Uint8Array<ArrayBuffer> {
  if (shape === "plane") {
    return polygonMask(
      projectedPlanePerimeter(rotationY)[0]!,
      frame.width,
      frame.height,
    );
  }
  const mask = new Uint8Array(frame.width * frame.height);
  for (let y = 0; y < frame.height; y += 1) {
    for (let x = 0; x < frame.width; x += 1) {
      const ray = cameraRay(x, y, frame.width, frame.height);
      if (intersectsShape(EYE, ray, shape)) mask[y * frame.width + x] = 1;
    }
  }
  return mask;
}

function cameraRay(x: number, y: number, width: number, height: number): Vec3 {
  const screenX =
    ((2 * (x + 0.5)) / width - 1) * FIELD_TANGENT * (width / height);
  const screenY = (1 - (2 * (y + 0.5)) / height) * FIELD_TANGENT;
  return normalize(
    add(scale(BACK, -1), add(scale(RIGHT, screenX), scale(UP, screenY))),
  );
}

function intersectsShape(
  origin: Vec3,
  direction: Vec3,
  shape: BaseShape,
): boolean {
  switch (shape) {
    case "cube":
      return intersectsBox(origin, direction, 1);
    case "sphere":
      return intersectsSphere(origin, direction, [0, 0, 0], 1);
    case "pill":
      return intersectsPill(origin, direction);
    case "cone": {
      // Intersect x² + z² = (1-y)²/4, clipped to -1 <= y <= 1,
      // and the radius-one cap. This oracle does not read generated triangles.
      const [x, y, z] = origin;
      const [dx, dy, dz] = direction;
      const cap = (-1 - y) / dy;
      if (cap > 0 && (x + cap * dx) ** 2 + (z + cap * dz) ** 2 <= 1)
        return true;
      const a = dx * dx + dz * dz - (dy * dy) / 4;
      const b = 2 * (x * dx + z * dz) + ((1 - y) * dy) / 2;
      const c = x * x + z * z - (1 - y) ** 2 / 4;
      const discriminant = b * b - 4 * a * c;
      if (discriminant < 0) return false;
      const roots =
        Math.abs(a) < 1e-12
          ? [-c / b]
          : [
              (-b - Math.sqrt(discriminant)) / (2 * a),
              (-b + Math.sqrt(discriminant)) / (2 * a),
            ];
      return roots.some(
        (distance) => distance > 0 && Math.abs(y + distance * dy) <= 1,
      );
    }
    case "plane":
      throw new Error("plane intersections use its projected polygon");
  }
}

function intersectsBox(
  origin: Vec3,
  direction: Vec3,
  halfExtent: number,
): boolean {
  let near = 0;
  let far = Number.POSITIVE_INFINITY;
  for (let axis = 0; axis < 3; axis += 1) {
    const o = origin[axis] ?? 0;
    const d = direction[axis] ?? 0;
    if (Math.abs(d) < 1e-12) {
      if (Math.abs(o) > halfExtent) return false;
      continue;
    }
    const a = (-halfExtent - o) / d;
    const b = (halfExtent - o) / d;
    near = Math.max(near, Math.min(a, b));
    far = Math.min(far, Math.max(a, b));
    if (near > far) return false;
  }
  return far >= near && far > 0;
}

function intersectsSphere(
  origin: Vec3,
  direction: Vec3,
  center: Vec3,
  radius: number,
): boolean {
  const offset = subtract(origin, center);
  const projection = dot(offset, direction);
  const discriminant =
    projection * projection - (dot(offset, offset) - radius * radius);
  return discriminant >= 0 && -projection + Math.sqrt(discriminant) > 0;
}

function intersectsPill(origin: Vec3, direction: Vec3): boolean {
  if (
    intersectsSphere(
      origin,
      direction,
      [0, PILL_BODY_HALF_HEIGHT, 0],
      PILL_RADIUS,
    ) ||
    intersectsSphere(
      origin,
      direction,
      [0, -PILL_BODY_HALF_HEIGHT, 0],
      PILL_RADIUS,
    )
  ) {
    return true;
  }
  const a = direction[0] ** 2 + direction[2] ** 2;
  const b = 2 * (origin[0] * direction[0] + origin[2] * direction[2]);
  const c = origin[0] ** 2 + origin[2] ** 2 - PILL_RADIUS ** 2;
  const discriminant = b * b - 4 * a * c;
  if (a <= 1e-12 || discriminant < 0) return false;
  const root = Math.sqrt(discriminant);
  for (const distance of [(-b - root) / (2 * a), (-b + root) / (2 * a)]) {
    const y = origin[1] + distance * direction[1];
    if (distance > 0 && Math.abs(y) <= PILL_BODY_HALF_HEIGHT) return true;
  }
  return false;
}

export function projectedContours(
  shape: BaseShape,
  rotationY: number,
  rings = 1,
): readonly (readonly Point[])[] {
  if (shape === "plane") {
    return [...projectedPlanePerimeter(rotationY), projectedNormal(rotationY)];
  }
  const world = contourWorldPolylines(shape, rings);
  return world.map((polyline) => polyline.map(projectPoint));
}

export function projectedPlanePerimeter(
  rotationY: number,
): readonly (readonly Point[])[] {
  const corners: Vec3[] = [
    [-PLANE_HALF_SIZE, -PLANE_HALF_SIZE, 0],
    [PLANE_HALF_SIZE, -PLANE_HALF_SIZE, 0],
    [PLANE_HALF_SIZE, PLANE_HALF_SIZE, 0],
    [-PLANE_HALF_SIZE, PLANE_HALF_SIZE, 0],
    [-PLANE_HALF_SIZE, -PLANE_HALF_SIZE, 0],
  ];
  return [
    corners.map((point) => projectPoint(transformPlanePoint(point, rotationY))),
  ];
}

export function projectedNormal(rotationY: number): readonly [Point, Point] {
  return [
    projectPoint(transformPlanePoint([0, 0, PLANE_NORMAL_OFFSET], rotationY)),
    projectPoint(
      transformPlanePoint(
        [0, 0, PLANE_NORMAL_OFFSET + PLANE_NORMAL_LENGTH],
        rotationY,
      ),
    ),
  ];
}

export function projectedArrowBounds(
  rotationY: number,
  length = PLANE_NORMAL_LENGTH,
  offset = PLANE_NORMAL_OFFSET,
  axis = 2,
): ImageSummary["bounds"] {
  const orient = (point: Vec3): Vec3 =>
    axis === 0
      ? [point[2], point[0], point[1]]
      : axis === 1
        ? [point[1], point[2], point[0]]
        : point;
  const points: Point[] = [];
  const shoulder = offset + length - Math.min(length / 4, PLANE_STROKE * 4);
  for (const [z, radius] of [
    [offset, PLANE_STROKE / 2],
    [shoulder, PLANE_STROKE / 2],
    [shoulder, PLANE_STROKE * 2],
  ] as const) {
    for (let side = 0; side < 64; side += 1) {
      const angle = (side * 2 * Math.PI) / 64;
      points.push(
        projectPoint(
          transformPlanePoint(
            orient([radius * Math.cos(angle), radius * Math.sin(angle), z]),
            rotationY,
          ),
        ),
      );
    }
  }
  points.push(
    projectPoint(
      transformPlanePoint(orient([0, 0, offset + length]), rotationY),
    ),
  );
  return pointBounds(points);
}

export function planeInteriorProbes(rotationY: number): readonly Point[] {
  const probes: readonly Vec3[] = [
    [-0.5, 0.45, 0],
    [0.5, -0.45, 0],
  ];
  return probes.map((point) =>
    projectPoint(transformPlanePoint(point, rotationY)),
  );
}

function transformPlanePoint(point: Vec3, rotationY: number): Vec3 {
  const cosine = Math.cos(rotationY);
  const sine = Math.sin(rotationY);
  return [
    cosine * point[0] + sine * point[2],
    point[1],
    -sine * point[0] + cosine * point[2],
  ];
}

function polygonMask(
  polygon: readonly Point[],
  width: number,
  height: number,
): Uint8Array<ArrayBuffer> {
  const mask = new Uint8Array(width * height);
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      if (pointInPolygon([x + 0.5, y + 0.5], polygon)) {
        mask[y * width + x] = 1;
      }
    }
  }
  return mask;
}

function pointInPolygon(point: Point, polygon: readonly Point[]): boolean {
  let inside = false;
  for (
    let current = 0, previous = polygon.length - 1;
    current < polygon.length;
    previous = current, current += 1
  ) {
    const a = polygon[current]!;
    const b = polygon[previous]!;
    if (
      a[1] > point[1] !== b[1] > point[1] &&
      point[0] < ((b[0] - a[0]) * (point[1] - a[1])) / (b[1] - a[1]) + a[0]
    ) {
      inside = !inside;
    }
  }
  return inside;
}

function pointBounds(points: readonly Point[]): ImageSummary["bounds"] {
  if (points.length === 0) return null;
  return {
    left: Math.floor(Math.min(...points.map(([x]) => x))),
    top: Math.floor(Math.min(...points.map(([, y]) => y))),
    right: Math.ceil(Math.max(...points.map(([x]) => x))),
    bottom: Math.ceil(Math.max(...points.map(([, y]) => y))),
  };
}

export function projectedVisibleLongitudeSeam(
  shape: "sphere" | "pill" | "cone",
): Point[] {
  if (shape === "cone") {
    return Array.from({ length: 127 }, (_, index) => {
      const fraction = (index + 1) / 128;
      return projectPoint([fraction, 1 - 2 * fraction, 0]);
    });
  }
  const halfHeight =
    shape === "sphere" ? 1 : PILL_BODY_HALF_HEIGHT + PILL_RADIUS;
  const bodyHalf = shape === "sphere" ? 0 : PILL_BODY_HALF_HEIGHT;
  const radius = shape === "sphere" ? 1 : PILL_RADIUS;
  const points: Point[] = [];
  for (let index = 1; index < 128; index += 1) {
    const y = halfHeight - (index / 128) * halfHeight * 2;
    const centerY = Math.max(-bodyHalf, Math.min(bodyHalf, y));
    const normalY = y - centerY;
    const x = Math.sqrt(Math.max(0, radius * radius - normalY * normalY));
    const point: Vec3 = [x, y, 0];
    const normal = normalize([x, normalY, 0]);
    if (dot(normal, subtract(EYE, point)) > 0) {
      points.push(projectPoint(point));
    }
  }
  return points;
}

function contourWorldPolylines(
  shape: BaseShape,
  rings: number,
): readonly (readonly Vec3[])[] {
  if (shape === "cone") {
    const lines: Vec3[][] = [
      [1, -1, 0],
      [0, -1, 1],
      [-1, -1, 0],
      [0, -1, -1],
    ].map((base) => [[0, 1, 0], base as unknown as Vec3]);
    for (let ring = 1; ring <= rings + 1; ring++) {
      const fraction = ring / (rings + 1);
      lines.push(circle((x, z) => [x, 1 - 2 * fraction, z], fraction));
    }
    return lines;
  }
  if (shape === "cube") {
    const lines: Vec3[][] = [];
    for (const axis of [0, 1, 2] as const) {
      const other = [0, 1, 2].filter((candidate) => candidate !== axis);
      for (const first of [-1, 1]) {
        for (const second of [-1, 1]) {
          const a = [0, 0, 0];
          const b = [0, 0, 0];
          a[axis] = -1;
          b[axis] = 1;
          a[other[0]!] = first;
          b[other[0]!] = first;
          a[other[1]!] = second;
          b[other[1]!] = second;
          lines.push([a as unknown as Vec3, b as unknown as Vec3]);
        }
      }
    }
    return lines;
  }
  if (shape === "sphere") {
    return [
      circle((a, b) => [a, b, 0], 1),
      circle((a, b) => [0, a, b], 1),
      circle((a, b) => [a, 0, b], 1),
    ];
  }
  if (shape === "plane") {
    throw new Error("plane contours use their transformed projection");
  }
  return [
    capsuleProfile((a, y) => [a, y, 0]),
    capsuleProfile((a, y) => [0, y, a]),
    circle((a, b) => [a, PILL_BODY_HALF_HEIGHT, b], PILL_RADIUS),
    circle((a, b) => [a, -PILL_BODY_HALF_HEIGHT, b], PILL_RADIUS),
  ];
}

function circle(map: (a: number, b: number) => Vec3, radius: number): Vec3[] {
  return closedCurve((angle) =>
    map(radius * Math.cos(angle), radius * Math.sin(angle)),
  );
}

function capsuleProfile(map: (across: number, y: number) => Vec3): Vec3[] {
  const points: Vec3[] = [];
  points.push(map(PILL_RADIUS, -PILL_BODY_HALF_HEIGHT));
  points.push(map(PILL_RADIUS, PILL_BODY_HALF_HEIGHT));
  for (let index = 1; index <= 64; index += 1) {
    const angle = (index * Math.PI) / 64;
    points.push(
      map(
        PILL_RADIUS * Math.cos(angle),
        PILL_BODY_HALF_HEIGHT + PILL_RADIUS * Math.sin(angle),
      ),
    );
  }
  points.push(map(-PILL_RADIUS, -PILL_BODY_HALF_HEIGHT));
  for (let index = 1; index <= 64; index += 1) {
    const angle = Math.PI + (index * Math.PI) / 64;
    points.push(
      map(
        PILL_RADIUS * Math.cos(angle),
        -PILL_BODY_HALF_HEIGHT + PILL_RADIUS * Math.sin(angle),
      ),
    );
  }
  return points;
}

function closedCurve(point: (angle: number) => Vec3): Vec3[] {
  return Array.from({ length: 193 }, (_, index) =>
    point((index * 2 * Math.PI) / 192),
  );
}

export function projectPoint(point: Vec3): Point {
  const relative = subtract(point, EYE);
  const depth = -dot(BACK, relative);
  const ndcX =
    dot(RIGHT, relative) /
    depth /
    (FIELD_TANGENT * (VIEWPORT.width / VIEWPORT.height));
  const ndcY = dot(UP, relative) / depth / FIELD_TANGENT;
  return [
    ((ndcX + 1) * VIEWPORT.width) / 2,
    ((1 - ndcY) * VIEWPORT.height) / 2,
  ];
}

export function stableMaskValue(
  mask: Uint8Array,
  width: number,
  x: number,
  y: number,
  expected: boolean,
): boolean {
  const value = expected ? 1 : 0;
  for (let offsetY = -2; offsetY <= 2; offsetY += 1) {
    for (let offsetX = -2; offsetX <= 2; offsetX += 1) {
      if (mask[(y + offsetY) * width + x + offsetX] !== value) return false;
    }
  }
  return true;
}

export function distanceToPolylines(
  point: Point,
  polylines: readonly (readonly Point[])[],
): number {
  let distance = Number.POSITIVE_INFINITY;
  for (const polyline of polylines) {
    for (let index = 1; index < polyline.length; index += 1) {
      distance = Math.min(
        distance,
        pointSegmentDistance(point, polyline[index - 1]!, polyline[index]!),
      );
    }
  }
  return distance;
}

function pointSegmentDistance(point: Point, a: Point, b: Point): number {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const lengthSquared = dx * dx + dy * dy;
  const amount =
    lengthSquared === 0
      ? 0
      : Math.max(
          0,
          Math.min(
            1,
            ((point[0] - a[0]) * dx + (point[1] - a[1]) * dy) / lengthSquared,
          ),
        );
  return Math.hypot(
    point[0] - (a[0] + amount * dx),
    point[1] - (a[1] + amount * dy),
  );
}

export function samplePolylines(
  polylines: readonly (readonly Point[])[],
  spacing: number,
): Point[] {
  const samples: Point[] = [];
  for (const polyline of polylines) {
    for (let index = 1; index < polyline.length; index += 1) {
      const a = polyline[index - 1]!;
      const b = polyline[index]!;
      const steps = Math.max(
        1,
        Math.ceil(Math.hypot(b[0] - a[0], b[1] - a[1]) / spacing),
      );
      for (let step = 0; step < steps; step += 1) {
        const amount = step / steps;
        samples.push([
          a[0] + (b[0] - a[0]) * amount,
          a[1] + (b[1] - a[1]) * amount,
        ]);
      }
    }
  }
  return samples;
}

export function neighborhoodHasForeground(
  mask: Uint8Array,
  width: number,
  height: number,
  x: number,
  y: number,
  radius: number,
): boolean {
  const centerX = Math.round(x - 0.5);
  const centerY = Math.round(y - 0.5);
  for (let offsetY = -radius; offsetY <= radius; offsetY += 1) {
    for (let offsetX = -radius; offsetX <= radius; offsetX += 1) {
      const sampleX = centerX + offsetX;
      const sampleY = centerY + offsetY;
      if (
        sampleX >= 0 &&
        sampleY >= 0 &&
        sampleX < width &&
        sampleY < height &&
        mask[sampleY * width + sampleX]
      ) {
        return true;
      }
    }
  }
  return false;
}

export function probeNeighborhood(
  mask: Uint8Array,
  width: number,
  height: number,
  point: Point,
  radius: number,
): readonly [number, number] {
  const centerX = Math.round(point[0] - 0.5);
  const centerY = Math.round(point[1] - 0.5);
  let pixels = 0;
  let foregroundPixels = 0;
  for (let offsetY = -radius; offsetY <= radius; offsetY += 1) {
    for (let offsetX = -radius; offsetX <= radius; offsetX += 1) {
      const x = centerX + offsetX;
      const y = centerY + offsetY;
      if (x < 0 || y < 0 || x >= width || y >= height) continue;
      pixels += 1;
      if (mask[y * width + x]) foregroundPixels += 1;
    }
  }
  return [pixels, foregroundPixels];
}

export function maskBounds(
  mask: Uint8Array,
  width: number,
  height: number,
): ImageSummary["bounds"] {
  let left = width;
  let top = height;
  let right = -1;
  let bottom = -1;
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      if (!mask[y * width + x]) continue;
      left = Math.min(left, x);
      top = Math.min(top, y);
      right = Math.max(right, x);
      bottom = Math.max(bottom, y);
    }
  }
  return right < 0 ? null : { left, top, right, bottom };
}

export function checkerColors(
  material: readonly number[],
): readonly [Vec3, Vec3, Vec3, Vec3] {
  return [
    [255, 0, 0],
    [0, 255, 0],
    [0, 0, 255],
    [0, 0, 0],
  ].map(
    (rgb) =>
      rgb.map((encoded, channel) =>
        linearToSrgb8(encoded === 0 ? 0 : (material[channel] ?? 0)),
      ) as unknown as Vec3,
  ) as unknown as readonly [Vec3, Vec3, Vec3, Vec3];
}

export function pixelRgb(frame: FrameCapture, index: number): Vec3 {
  const pixels = new Uint8Array(frame.pixels);
  const offset = index * 4;
  return [
    pixels[offset] ?? 0,
    pixels[offset + 1] ?? 0,
    pixels[offset + 2] ?? 0,
  ];
}

export function maximumDifference(
  actual: readonly number[],
  expected: readonly number[],
): number {
  return Math.max(
    ...actual.map((value, index) =>
      Math.abs(value - (expected[index] ?? Number.NaN)),
    ),
  );
}

function normalize(value: Vec3): Vec3 {
  const length = Math.hypot(...value);
  return [value[0] / length, value[1] / length, value[2] / length];
}

function add(a: Vec3, b: Vec3): Vec3 {
  return [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
}

function subtract(a: Vec3, b: Vec3): Vec3 {
  return [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
}

function scale(value: Vec3, amount: number): Vec3 {
  return [value[0] * amount, value[1] * amount, value[2] * amount];
}

function dot(a: Vec3, b: Vec3): number {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}

function cross(a: Vec3, b: Vec3): Vec3 {
  return [
    a[1] * b[2] - a[2] * b[1],
    a[2] * b[0] - a[0] * b[2],
    a[0] * b[1] - a[1] * b[0],
  ];
}

export function linearToSrgb8(value: number): number {
  const encoded =
    value <= 0.003_130_8 ? value * 12.92 : 1.055 * value ** (1 / 2.4) - 0.055;
  return Math.round(Math.min(1, Math.max(0, encoded)) * 255);
}
