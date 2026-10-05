import {
  clientAssetSource,
  type AnimationWorldClient,
  type AnimationTrack,
  type ClientAssetSource,
  type ComponentSnapshot,
  type Inspection,
} from "@ipp/client";
import { componentFields, successfulBatch } from "./shared/commands.js";
import {
  CHART_CATALOG,
  CHART_RING,
  CHART_SURFACE,
  rotateChartPoint,
} from "./catalog.js";

export type CameraPose = Record<
  "x" | "y" | "z" | "qx" | "qy" | "qz" | "qw",
  number
>;
export interface ChartFocus {
  readonly chart: string;
  readonly controller: bigint;
  readonly duration: 2;
  readonly startTime: number;
  readonly from: CameraPose;
  readonly to: CameraPose;
}

function lookAt(eye: readonly number[], target: readonly number[]): CameraPose {
  const yaw = Math.atan2(eye[0]! - target[0]!, eye[2]! - target[2]!) / 2;
  const pitch =
    -Math.atan2(
      eye[1]! - target[1]!,
      Math.hypot(eye[0]! - target[0]!, eye[2]! - target[2]!),
    ) / 2;
  return {
    x: eye[0]!,
    y: eye[1]!,
    z: eye[2]!,
    qx: Math.sin(pitch) * Math.cos(yaw),
    qy: Math.cos(pitch) * Math.sin(yaw),
    qz: -Math.sin(pitch) * Math.sin(yaw),
    qw: Math.cos(pitch) * Math.cos(yaw),
  };
}

function framedTarget(
  centre: readonly number[],
  direction: readonly number[],
  half: readonly number[],
  aspect: number,
  yaw = 0,
) {
  const length = Math.hypot(...direction);
  const back = direction.map((value) => value / length);
  const horizontal = Math.hypot(back[0]!, back[2]!);
  const right = [back[2]! / horizontal, 0, -back[0]! / horizontal];
  const up = [
    back[1]! * right[2]!,
    back[2]! * right[0]! - back[0]! * right[2]!,
    -back[1]! * right[0]!,
  ];
  // Fit every corner in camera space, with room for labels and chart axes.
  const tangent = Math.tan(Math.PI / 6);
  let distance = 5;
  for (const x of [-half[0]!, half[0]!])
    for (const y of [-half[1]!, half[1]!])
      for (const z of [-half[2]!, half[2]!]) {
        const corner = rotateChartPoint([x, y, z], yaw);
        const dot = (axis: readonly number[]) =>
          corner.reduce((sum, value, index) => sum + value * axis[index]!, 0);
        distance = Math.max(
          distance,
          dot(back) + (1.18 * Math.abs(dot(right))) / (tangent * aspect),
          dot(back) + (1.18 * Math.abs(dot(up))) / tangent,
        );
      }
  const eye = centre.map((value, index) => value + back[index]! * distance);
  return { pose: lookAt(eye, centre), distance };
}

export function chartCameraTarget(id: string, aspect = 1) {
  if (!Number.isFinite(aspect) || aspect <= 0)
    throw new Error("Chart camera requires a positive viewport aspect ratio");
  if (id === "center")
    return {
      pose: lookAt(CHART_RING.center, CHART_CATALOG[0]!.center),
      distance: CHART_RING.radius,
    };
  if (id === "overview") {
    const extent = CHART_RING.radius + 8;
    return framedTarget(
      CHART_RING.center,
      [0, 55, 75],
      [extent, 8, extent],
      aspect,
    );
  }
  const chart = CHART_CATALOG.find((chart) => chart.id === id);
  if (!chart) throw new Error(`Unknown chart: ${id}`);
  const canvas = chart.component.endsWith("2d");
  const pie = chart.component === "PlotPie3d";
  const width = canvas ? CHART_SURFACE.width : Number(chart.frame.width);
  const height = canvas ? CHART_SURFACE.height : Number(chart.frame.height);
  const depth = canvas ? 0 : Number(chart.frame.depth);
  return framedTarget(
    chart.center,
    rotateChartPoint(canvas ? [0, 0, 1] : [0, 7, 13], chart.yaw),
    canvas
      ? [
          width / 2,
          height / 2,
          CHART_RING.radius * (1 - Math.cos(width / (2 * CHART_RING.radius))),
        ]
      : [width / 2 + 0.5, pie ? 3 : height / 2 + 1, depth / 2 + 0.5],
    aspect,
    chart.yaw,
  );
}

function multiply(
  a: readonly number[],
  b: readonly number[],
): [number, number, number, number] {
  const [ax, ay, az, aw] = a as [number, number, number, number],
    [bx, by, bz, bw] = b as [number, number, number, number];
  return [
    aw * bx + ax * bw + ay * bz - az * by,
    aw * by - ax * bz + ay * bw + az * bx,
    aw * bz + ax * by - ay * bx + az * bw,
    aw * bw - ax * bx - ay * by - az * bz,
  ];
}

function rotation(pose: CameraPose) {
  return [pose.qx, pose.qy, pose.qz, pose.qw] as const;
}
function fields(
  inspection: Inspection,
  entity: bigint,
  component: number,
): ComponentSnapshot["fields"] {
  const value = inspection.entities
    .find((item) => item.id === entity)
    ?.components.find((item) => item.component === component)?.fields;
  if (!value) throw new Error("Chart camera was removed");
  return value;
}

/** Camera motion is sampled by AnimationSystem, never a browser animation loop. */
export class ChartCamera {
  focus: ChartFocus | null = null;
  // The chart client is the sole manual writer. Re-anchor after Host-owned motion.
  private authoredPose: CameraPose | undefined;
  private endpoint:
    | { pose: CameraPose; distance: number; sx: number; sy: number; sz: number }
    | undefined;
  private completed = false;
  private readonly stopPlaybackObservation: () => void;
  private asset: ClientAssetSource | undefined;
  private nextAsset = 720000n;
  aspect = 1;
  constructor(
    readonly client: AnimationWorldClient,
    readonly entity: bigint,
  ) {
    this.stopPlaybackObservation = client.onPlaybackEvent((event) => {
      if (
        event.controller.id === this.focus?.controller &&
        event.kind === "completed"
      )
        this.completed = true;
    });
  }

  pose(inspection: Inspection): CameraPose {
    const value = fields(
      inspection,
      this.entity,
      this.client.components.Transform!.id,
    );
    return Object.fromEntries(
      ["x", "y", "z", "qx", "qy", "qz", "qw"].map((name) => [
        name,
        Number(value[name]),
      ]),
    ) as CameraPose;
  }

  inspect() {
    return this.client.inspectPage({
      collection: "entities",
      target: this.entity,
    });
  }

  invalidate() {
    this.authoredPose = undefined;
  }

  async write(pose: CameraPose, distance?: number) {
    successfulBatch(
      await this.client.batch(
        componentFields(this.client, "Transform", pose)
          .map((field) => ({
            kind: "setField" as const,
            entity: { kind: "handle" as const, id: this.entity },
            component: this.client.components.Transform!.id,
            field,
          }))
          .concat(
            distance === undefined
              ? []
              : componentFields(this.client, "Camera", {
                  focus_distance: distance,
                }).map((field) => ({
                  kind: "setField" as const,
                  entity: { kind: "handle" as const, id: this.entity },
                  component: this.client.components.Camera!.id,
                  field,
                })),
          ),
      ),
    );
    this.authoredPose = { ...pose };
  }

  /** Client-owned look gestures author orientation without moving the camera eye. */
  async turn(yaw: number, pitch: number) {
    if (!Number.isFinite(yaw) || !Number.isFinite(pitch))
      throw new Error("Camera turn requires finite yaw and pitch");
    const pose = this.authoredPose ?? this.pose(await this.inspect());
    const q = multiply(
      multiply([0, Math.sin(yaw / 2), 0, Math.cos(yaw / 2)], rotation(pose)),
      [Math.sin(pitch / 2), 0, 0, Math.cos(pitch / 2)],
    );
    const length = Math.hypot(...q);
    successfulBatch(
      await this.client.batch(
        componentFields(this.client, "Transform", {
          qx: q[0] / length,
          qy: q[1] / length,
          qz: q[2] / length,
          qw: q[3] / length,
        }).map((field) => ({
          kind: "setField",
          entity: { kind: "handle", id: this.entity },
          component: this.client.components.Transform!.id,
          field,
        })),
      ),
    );
    this.authoredPose = {
      ...pose,
      qx: q[0] / length,
      qy: q[1] / length,
      qz: q[2] / length,
      qw: q[3] / length,
    };
  }

  async cancel() {
    if (!this.focus) {
      if (this.asset) await this.client.releaseAsset(this.asset);
      this.asset = undefined;
      return;
    }
    const focus = this.focus;
    const camera = this.client.components.Camera!;
    let sampled = this.completed ? this.endpoint : undefined;
    if (!sampled) {
      await this.client.controlAnimationController(focus.controller, {
        action: "pause",
      });
      const inspection = await this.inspect();
      const transform = fields(
        inspection,
        this.entity,
        this.client.components.Transform!.id,
      );
      sampled = {
        pose: { ...this.pose(inspection) },
        distance: Number(
          fields(inspection, this.entity, camera.id).focus_distance,
        ),
        sx: Number(transform.sx),
        sy: Number(transform.sy),
        sz: Number(transform.sz),
      };
    }
    const { pose, distance } = sampled;
    // Replace the sampled Transform before invalidation can withdraw its contribution.
    // Camera keeps its incarnation so the root output remains valid.
    successfulBatch(
      await this.client.batch([
        {
          kind: "insertComponent",
          entity: { kind: "handle", id: this.entity },
          component: this.client.components.Transform!.id,
          fields: componentFields(this.client, "Transform", {
            ...pose,
            sx: sampled.sx,
            sy: sampled.sy,
            sz: sampled.sz,
          }),
          adopt: false,
        },
      ]),
    );
    await this.client.deleteAnimationController(focus.controller);
    this.focus = null;
    this.endpoint = undefined;
    this.completed = false;
    successfulBatch(
      await this.client.batch(
        componentFields(this.client, "Camera", {
          focus_distance: distance,
        }).map((field) => ({
          kind: "setField",
          entity: { kind: "handle", id: this.entity },
          component: camera.id,
          field,
        })),
      ),
    );
    if (this.asset) await this.client.releaseAsset(this.asset);
    this.asset = undefined;
    this.authoredPose = pose;
  }

  async move(id: string, current: () => boolean = () => true) {
    const target = chartCameraTarget(id, this.aspect);
    await this.cancel();
    if (!current()) return;
    const inspection = await this.inspect();
    if (!current()) return;
    const from = { ...this.pose(inspection) },
      to = target.pose;
    const transform = this.client.components.Transform!,
      camera = this.client.components.Camera!;
    const scalar = (
      component: number,
      offset: number,
      delta: number,
    ): AnimationTrack => ({
      property: { component, offsets: [offset] },
      keys: [
        {
          time: 0,
          value: { kind: "f32", value: 0 },
          interpolation: {
            kind: "bezier",
            time1: 2 / 3,
            value1: { kind: "f32", value: 0 },
            time2: 4 / 3,
            value2: { kind: "f32", value: delta },
          },
        },
        { time: 2, value: { kind: "f32", value: delta } },
      ],
    });
    const currentDistance = Number(
      fields(inspection, this.entity, camera.id).focus_distance,
    );
    const sampledTransform = fields(inspection, this.entity, transform.id);
    const tracks: AnimationTrack[] = (["x", "y", "z"] as const).map((name) =>
      scalar(
        transform.id,
        transform.fields[name]!.offset,
        to[name] - from[name],
      ),
    );
    tracks.push(
      scalar(
        camera.id,
        camera.fields.focus_distance!.offset,
        target.distance - currentDistance,
      ),
    );
    const origin = rotation(from),
      destination = rotation(to);
    let relative = multiply(
      [-origin[0], -origin[1], -origin[2], origin[3]],
      destination,
    );
    if (relative[3] < 0)
      relative = relative.map((value) => -value) as typeof relative;
    const angle = Math.acos(Math.max(-1, Math.min(1, relative[3]))),
      sin = Math.sin(angle);
    tracks.push({
      property: {
        component: transform.id,
        offsets: ["qx", "qy", "qz", "qw"].map(
          (name) => transform.fields[name]!.offset,
        ),
      },
      keys: Array.from({ length: 33 }, (_, index) => {
        const t = index / 32,
          eased = t * t * (3 - 2 * t),
          ratio = Math.abs(sin) < 1e-7 ? eased : Math.sin(eased * angle) / sin;
        return {
          time: t * 2,
          value: {
            kind: "rotation",
            value: [
              relative[0] * ratio,
              relative[1] * ratio,
              relative[2] * ratio,
              Math.cos(eased * angle),
            ],
          },
        };
      }),
    });
    const asset = clientAssetSource(this.client.session, 10, this.nextAsset++);
    this.asset = asset;
    try {
      await this.client.registerAsset(
        asset,
        this.client.encodeAnimationClip({ duration: 2, tracks }).buffer,
      );
      if (!current()) return;
      const controller = await this.client.createAnimationController({
        drivers: tracks.map((track, index) => ({
          source: asset.source,
          track: index,
          target: this.entity,
          property: track.property!,
        })),
      });
      this.focus = {
        chart: id,
        controller,
        duration: 2,
        startTime: inspection.time,
        from,
        to,
      };
      this.endpoint = {
        pose: to,
        distance: target.distance,
        sx: Number(sampledTransform.sx),
        sy: Number(sampledTransform.sy),
        sz: Number(sampledTransform.sz),
      };
      this.completed = false;
      this.invalidate();
      if (!current()) return;
      // Registration acknowledges ownership; decoding is a separate Host task.
      const deadline = performance.now() + 30_000;
      for (;;) {
        const resources = await this.client.inspectPage({
          collection: "resources",
        });
        if (!current()) return;
        const resource = resources.resources.find(
          (item) => item.source === asset.source,
        );
        if (resource?.status === "failed")
          throw new Error(
            `Camera focus resource failed: ${resource.error ?? asset.source}`,
          );
        if (resource?.status === "loaded" && resource.representation.decoded)
          break;
        if (performance.now() > deadline)
          throw new Error("Camera focus animation did not become ready");
        await this.client.waitForFrame(resources.tick);
        if (!current()) return;
      }
      await this.client.controlAnimationController(controller, {
        action: "play",
      });
      if (!current()) return;
      const started = await this.client.inspectPage({
        collection: "controllers",
        target: controller,
      });
      if (!current()) return;
      const clock =
        started.controllers?.find((item) => item.id === controller)?.time ?? 0;
      this.focus = { ...this.focus, startTime: started.time - clock };
    } catch (error) {
      try {
        await this.cancel();
      } catch (cleanup) {
        throw new AggregateError(
          [error, cleanup],
          "Camera focus cleanup is incomplete",
        );
      }
      throw error;
    }
  }

  async close() {
    try {
      await this.cancel();
    } finally {
      this.stopPlaybackObservation();
    }
  }
}
