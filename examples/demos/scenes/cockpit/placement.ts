import { carouselStep, COCKPIT_PANE_COUNT } from "./carousel.js";

/** Placement derived from the authored cockpit mounts and fixed Camera. */
export const TRANSFORM_FIELDS = [
  "x",
  "y",
  "z",
  "qx",
  "qy",
  "qz",
  "qw",
  "sx",
  "sy",
  "sz",
] as const;

export type Pose = Record<(typeof TRANSFORM_FIELDS)[number], number>;

export interface ScreenMount {
  readonly pose: Pose;
  readonly extent: readonly [number, number];
}

export interface CurvedScreenMount extends ScreenMount {
  readonly curvature: number;
}

export function readPose(fields: Readonly<Record<string, unknown>>): Pose {
  return Object.fromEntries(
    TRANSFORM_FIELDS.map((name) => [
      name,
      Number(fields[name] ?? (name === "qw" || name.startsWith("s") ? 1 : 0)),
    ]),
  ) as Pose;
}

function rotate(point: readonly number[], pose: Pose, inverse = false) {
  const sign = inverse ? -1 : 1;
  const x = pose.qx * sign;
  const y = pose.qy * sign;
  const z = pose.qz * sign;
  const [px, py, pz] = point as readonly [number, number, number];
  const ux = 2 * (y * pz - z * py);
  const uy = 2 * (z * px - x * pz);
  const uz = 2 * (x * py - y * px);
  return [
    px + pose.qw * ux + y * uz - z * uy,
    py + pose.qw * uy + z * ux - x * uz,
    pz + pose.qw * uz + x * uy - y * ux,
  ] as const;
}

/** Every panel center and tangent lies on one cylinder around the camera axis. */
export function carouselScreenMounts(
  _mounts: readonly ScreenMount[],
  radius: number,
  count = COCKPIT_PANE_COUNT,
): readonly CurvedScreenMount[] {
  const width = 960 / 200;
  const height = 800 / 200;
  const step = carouselStep(width, radius);
  return Array.from({ length: count }, (_, index) => {
    const angle = index * step;
    return {
      extent: [width, height] as const,
      curvature: -1 / radius,
      pose: {
        x: radius * Math.sin(angle),
        y: 0.65,
        z: -radius * Math.cos(angle),
        qx: 0,
        qy: Math.sin(-angle / 2),
        qz: 0,
        qw: Math.cos(angle / 2),
        sx: 1,
        sy: 1,
        sz: 1,
      },
    };
  });
}

/** Ray angle comes from runtime projection; this only changes world to camera axes. */
export function cameraRayAngle(point: readonly number[], camera: Pose) {
  const local = rotate(
    [point[0]! - camera.x, point[1]! - camera.y, point[2]! - camera.z],
    camera,
    true,
  );
  return Math.atan2(local[0], -local[2]);
}

export function cameraForward(camera: Pose) {
  return rotate([0, 0, -1], camera);
}
