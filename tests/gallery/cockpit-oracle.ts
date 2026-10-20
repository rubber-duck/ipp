/** Geometry observations independent of the application's placement helpers. */
import assert from "node:assert/strict";
import type { Inspection } from "@ipp/client";

export const COCKPIT_ANCHORS = [
  "cockpit-screen-0",
  "cockpit-screen-1",
  "cockpit-screen-2",
  "cockpit-screen-3",
  "cockpit-pulse-surface",
  "cockpit-scanner-surface",
] as const;

/** Scene Camera rectangle, normalized to the completed root image. */
export type CockpitViewport = readonly [number, number, number, number];

/** Map the independently measured 1672 × 941 camera plate into its actual slot. */
export function cockpitPlate(
  frame: { width: number; height: number },
  viewport: CockpitViewport,
) {
  const width = viewport[2] * frame.width;
  const height = viewport[3] * frame.height;
  const scale = Math.min(width, height) / 941;
  return {
    scale,
    left: viewport[0] * frame.width + (width - 1672 * scale) / 2,
    top: viewport[1] * frame.height + (height - 941 * scale) / 2,
  };
}

export function cockpitFields(
  snapshot: Inspection,
  name: string,
  component: number,
) {
  const entity = snapshot.entities.find(
    (value) => value.metadata.symbolicId === name,
  );
  assert.ok(entity, `${name} exists`);
  const result = entity.components.find(
    (value) => value.component === component,
  )?.fields;
  assert.ok(result, `${name} contains component ${component}`);
  return result;
}

export function rotate(
  point: readonly number[],
  pose: Readonly<Record<string, unknown>>,
  inverse = false,
) {
  const sign = inverse ? -1 : 1;
  const q = [
    Number(pose.qx) * sign,
    Number(pose.qy) * sign,
    Number(pose.qz) * sign,
  ];
  const w = Number(pose.qw);
  const cross = (a: readonly number[], b: readonly number[]) => [
    a[1]! * b[2]! - a[2]! * b[1]!,
    a[2]! * b[0]! - a[0]! * b[2]!,
    a[0]! * b[1]! - a[1]! * b[0]!,
  ];
  const u = cross(q, point).map((value) => value * 2);
  const v = cross(q, u);
  return point.map((value, index) => value + w * u[index]! + v[index]!);
}

function globalPose(
  local: Readonly<Record<string, unknown>>,
  parent: Readonly<Record<string, unknown>>,
): Record<string, unknown> {
  const position = rotate(
    [Number(local.x), Number(local.y), Number(local.z)],
    parent,
  );
  const [ax, ay, az, aw] = [parent.qx, parent.qy, parent.qz, parent.qw].map(
    Number,
  ) as [number, number, number, number];
  const [bx, by, bz, bw] = [local.qx, local.qy, local.qz, local.qw].map(
    Number,
  ) as [number, number, number, number];
  return {
    ...local,
    x: Number(parent.x) + position[0]!,
    y: Number(parent.y) + position[1]!,
    z: Number(parent.z) + position[2]!,
    qx: aw * bx + ax * bw + ay * bz - az * by,
    qy: aw * by - ax * bz + ay * bw + az * bx,
    qz: aw * bz + ax * by - ay * bx + az * bw,
    qw: aw * bw - ax * bx - ay * by - az * bz,
  };
}

export function surfaceSample(
  snapshot: Inspection,
  ids: Readonly<Record<string, number>>,
  panel: number,
  u: number,
  v: number,
) {
  const name = COCKPIT_ANCHORS[panel]!;
  const entity = snapshot.entities.find(
    (value) => value.metadata.symbolicId === name,
  )!;
  const flat = entity.components.find(
    (value) => value.component === ids.FlatSurface,
  );
  const curved = entity.components.find(
    (value) => value.component === ids.CylinderSurface,
  );
  assert.equal(
    Number(Boolean(flat)) + Number(Boolean(curved)),
    1,
    `${name} has exactly one Surface provider`,
  );
  const surface = (flat ?? curved)!.fields;
  const localPose = cockpitFields(snapshot, name, ids.Transform!);
  let pose: Readonly<Record<string, unknown>> = localPose;
  for (const parent of panel < 4
    ? ["cockpit-carousel", "cockpit-carousel-mount", "cockpit-bridge"]
    : ["cockpit-bridge"]) {
    pose = globalPose(pose, cockpitFields(snapshot, parent, ids.Transform!));
  }
  const x = (u - 0.5) * Number(surface.width);
  const y = (0.5 - v) * Number(surface.height);
  const k = curved ? Number(surface.curvature) : 0;
  const local =
    k === 0 ? [x, y, 0] : [Math.sin(k * x) / k, y, (Math.cos(k * x) - 1) / k];
  const position = rotate(
    local.map(
      (value, index) => value * Number(pose[["sx", "sy", "sz"][index]!]),
    ),
    pose,
  ).map((value, index) => value + Number(pose[["x", "y", "z"][index]!]));
  return {
    provider: flat ? "flat" : "curved",
    position,
    normal: rotate([Math.sin(k * x), 0, Math.cos(k * x)], pose),
    pose,
    surface,
    curvature: k,
  };
}

export function cameraCoordinates(
  point: readonly number[],
  camera: Readonly<Record<string, unknown>>,
) {
  return rotate(
    point.map(
      (value, index) => value - Number(camera[["x", "y", "z"][index]!]),
    ),
    camera,
    true,
  );
}

function near(actual: number, expected: number, label: string) {
  assert.ok(
    Math.abs(actual - expected) < 0.00002,
    `${label}: ${actual} ≈ ${expected}`,
  );
}

export function assertCylinder(
  scene: Inspection,
  ids: Readonly<Record<string, number>>,
  radius: number,
) {
  const camera = globalPose(
    cockpitFields(scene, "cockpit-carousel-mount", ids.Transform!),
    cockpitFields(scene, "cockpit-bridge", ids.Transform!),
  );
  const spans: { start: number; end: number; top: number; bottom: number }[] =
    [];
  for (let panel = 0; panel < 4; panel++) {
    const center = surfaceSample(scene, ids, panel, 0.5, 0.5);
    near(
      Number(center.surface.width),
      960 / 200,
      "screen logical width maps to metres",
    );
    near(
      Number(center.surface.height),
      800 / 200,
      "screen logical height maps to metres",
    );
    near(center.curvature, -1 / radius, "all panels share inward curvature");
    for (const scale of ["sx", "sy", "sz"])
      near(
        Number(center.pose[scale]),
        1,
        "curved panel keeps the cylinder's metric",
      );
    for (const u of [0, 0.5, 1])
      for (const v of [0, 0.5, 1]) {
        const sample = surfaceSample(scene, ids, panel, u, v);
        const point = cameraCoordinates(sample.position, camera);
        const normal = rotate(sample.normal, camera, true);
        near(
          Math.hypot(point[0]!, point[2]!),
          radius,
          "every sample is on the bridge-mounted cylinder",
        );
        near(
          normal[1]!,
          0,
          "the shared cylinder axis follows the bridge mount",
        );
        near(
          (normal[0]! * -point[0]! + normal[2]! * -point[2]!) / radius,
          1,
          "front normals face the viewer's axis",
        );
      }
    const left = cameraCoordinates(
      surfaceSample(scene, ids, panel, 0, 0.5).position,
      camera,
    );
    const right = cameraCoordinates(
      surfaceSample(scene, ids, panel, 1, 0.5).position,
      camera,
    );
    spans.push({
      start: Math.atan2(left[0]!, -left[2]!),
      end: Math.atan2(right[0]!, -right[2]!),
      top: cameraCoordinates(
        surfaceSample(scene, ids, panel, 0.5, 0).position,
        camera,
      )[1]!,
      bottom: cameraCoordinates(
        surfaceSample(scene, ids, panel, 0.5, 1).position,
        camera,
      )[1]!,
    });
  }
  const gaps = spans.slice(1).map((span, index) => {
    const delta = span.start - spans[index]!.end;
    return radius * Math.atan2(Math.sin(delta), Math.cos(delta));
  });
  assert.ok(
    gaps.every((gap) => Math.abs(gap - 0.07) < 0.0001),
    `four spans preserve their metric gaps: ${gaps}`,
  );
  for (const span of spans.slice(1)) {
    near(span.top, spans[0]!.top, "top rims are aligned");
    near(span.bottom, spans[0]!.bottom, "bottom rims are aligned");
  }
}

export function assertFlat(
  scene: Inspection,
  ids: Readonly<Record<string, number>>,
  radius: number,
) {
  const camera = globalPose(
    cockpitFields(scene, "cockpit-carousel-mount", ids.Transform!),
    cockpitFields(scene, "cockpit-bridge", ids.Transform!),
  );
  for (let panel = 0; panel < 6; panel++) {
    const sample = surfaceSample(scene, ids, panel, 0.5, 0.5);
    assert.equal(
      sample.provider,
      "flat",
      "Flat uses actual tangent FlatSurface providers",
    );
    if (panel >= 4) continue;
    near(
      Number(sample.surface.width),
      960 / 200,
      "flat screen preserves logical width",
    );
    near(
      Number(sample.surface.height),
      800 / 200,
      "flat screen preserves logical height",
    );
    const point = cameraCoordinates(sample.position, camera);
    near(
      Math.hypot(point[0]!, point[2]!),
      radius,
      "flat tangent center uses authored mount distance",
    );
    const normal = rotate(sample.normal, camera, true);
    near(
      (normal[0]! * -point[0]! + normal[2]! * -point[2]!) / radius,
      1,
      "flat tangent faces camera axis",
    );
  }
}
