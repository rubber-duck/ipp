import type { BoundingShape, SurfaceFacing } from "@ipp/client";
import {
  BoundingGeometry,
  CustomMaterial,
  Children,
  Entity,
  MeshInstance,
  PickingGeometry,
  Transform,
  assetRef,
} from "@ipp/react";
import { encodeBoundingShape } from "@ipp/host-contract";
import { SHIELD_CONTENT_RECT } from "./dashboard.js";
import {
  CANVAS_HEIGHT,
  CANVAS_WIDTH,
  UNITS_PER_METRE,
  SURFACE_RADIUS,
  REST_LAYER_SPACING,
} from "./presentation.js";
import { PANEL_SCALE } from "./projector.js";
import type { GuiSurfaceShape } from "./scene.js";

/** Symbolic ID the scene resolves to name the shield as a GUI input blocker. */
export const SHIELD_ENTITY = "gui-input-shield";

export const SHIELD_MESH = "ipp://mesh/cube?width=1&height=1&length=1";

/** World metres above the highest protected shell point, and overlap behind
 * its lowest point. Side walls close the oblique gap down to the Surface. */
const SHIELD_FRONT = 0.1;
const SHIELD_OVERLAP = 0.005;

/** The unit cube's picking box; the Transform scale sizes it with the mesh. */
const SHIELD_PICKING: BoundingShape = {
  type: "box",
  min: [-0.5, -0.5, -0.5],
  max: [0.5, 0.5, 0.5],
};

/** Fixed shell recipes selected by the shared Host animation clip. */
export const SHIELD_SURFACES = [
  ["flat", "outside"],
  ["cylinder", "outside"],
  ["cylinder", "inside"],
  ["sphere", "outside"],
  ["sphere", "inside"],
] as const;
export const SHIELD_FIELDS = ["x", "y", "z", "sx", "sy", "sz"] as const;

export function shieldTrackStart(
  shape: GuiSurfaceShape,
  facing: SurfaceFacing,
): number {
  const index = SHIELD_SURFACES.findIndex(
    ([s, f]) => s === shape && (s === "flat" || f === facing),
  );
  return 1 + index * SHIELD_FIELDS.length;
}

/** A conservative five-sided tangent cover, in its Surface parent's local metres.
 * Its visible mesh and picking box share exactly this pose. Shell movement
 * is linear in spacing, so one Host controller drives it with the Surface. */
export function shieldPose(
  shape: GuiSurfaceShape,
  facing: SurfaceFacing,
  spacing: number,
) {
  const k =
    shape === "flat" ? 0 : (facing === "inside" ? -1 : 1) / SURFACE_RADIUS;
  const depth = 3 * spacing;
  const sample = (u: number, v: number) => {
    const x = (u - CANVAS_WIDTH / 2) / UNITS_PER_METRE;
    const y = (CANVAS_HEIGHT / 2 - v) / UNITS_PER_METRE;
    if (k === 0) return { p: [x, y, depth], n: [0, 0, 1] };
    const angle = k * (shape === "sphere" ? Math.hypot(x, y) : Math.abs(x));
    const sinc = angle === 0 ? 1 : Math.sin(angle) / angle;
    const cosine = Math.cos(angle);
    const n = [k * x * sinc, shape === "sphere" ? k * y * sinc : 0, cosine];
    return {
      p: [
        x * sinc + depth * n[0]!,
        (shape === "sphere" ? y * sinc : y) + depth * n[1]!,
        (cosine - 1) / k + depth * n[2]!,
      ],
      n,
    };
  };
  const [u, v, width, height] = SHIELD_CONTENT_RECT;
  const { p, n } = sample(u + width / 2, v + height / 2);
  // The principal chart is far from the antipodal singularity. This rotation
  // takes +Z to the sampled normal while keeping tangent glass axes stable.
  const qw = Math.sqrt((1 + n[2]!) / 2);
  const qx = -n[1]! / (2 * qw);
  const qy = n[0]! / (2 * qw);
  const tangentX = [1 - 2 * qy * qy, 2 * qx * qy, -2 * qw * qy];
  const tangentY = [2 * qx * qy, 1 - 2 * qx * qx, 2 * qw * qx];
  const corners = [
    [u, v],
    [u + width, v],
    [u, v + height],
    [u + width, v + height],
  ].map(([x, y]) => sample(x!, y!).p.map((value, axis) => value - p[axis]!));
  const extent = (axis: number[]) =>
    2 *
    Math.max(
      ...corners.map((corner) =>
        Math.abs(
          corner.reduce((sum, value, index) => sum + value * axis[index]!, 0),
        ),
      ),
    );
  const normalOffsets = corners.map((corner) =>
    corner.reduce((sum, value, axis) => sum + value * n[axis]!, 0),
  );
  const back = Math.min(0, ...normalOffsets) - SHIELD_OVERLAP / PANEL_SCALE;
  const front = Math.max(0, ...normalOffsets) + SHIELD_FRONT / PANEL_SCALE;
  const middle = (back + front) / 2;
  return {
    x: p[0]! + middle * n[0]!,
    y: p[1]! + middle * n[1]!,
    z: p[2]! + middle * n[2]!,
    qx,
    qy,
    qz: 0,
    qw,
    sx: extent(tangentX),
    sy: extent(tangentY),
    sz: front - back,
  };
}

/** Relative motion inside the unanimated tangent frame. Transform tracks
 * contribute from their clip-start sample, so their start is the neutral pose. */
export function shieldMotion(
  shape: GuiSurfaceShape,
  facing: SurfaceFacing,
  spacing: number,
) {
  const base = shieldPose(shape, facing, REST_LAYER_SPACING);
  const pose = shieldPose(shape, facing, spacing);
  const { qx, qy, qw } = base;
  const axes = [
    [1 - 2 * qy * qy, 2 * qx * qy, -2 * qw * qy],
    [2 * qx * qy, 1 - 2 * qx * qx, 2 * qw * qx],
    [2 * qw * qy, -2 * qw * qx, 1 - 2 * (qx * qx + qy * qy)],
  ];
  const delta = [pose.x - base.x, pose.y - base.y, pose.z - base.z];
  const local = axes.map((axis) =>
    axis.reduce((sum, value, index) => sum + value * delta[index]!, 0),
  );
  return {
    x: local[0]! / base.sx,
    y: local[1]! / base.sy,
    z: local[2]! / base.sz,
    sx: pose.sx / base.sx,
    sy: pose.sy / base.sy,
    sz: pose.sz / base.sz,
  };
}

/** A front cap and four side walls over the Workbench's PURGE control. Armed glass is
 * named as an input blocker; lifted glass remains visible without blocking.
 * The shield is a child of the Surface parent and follows rank3, including
 * curved shell position, orientation and size, on the same Host controller. */
export function InputShield({
  armed,
  shape,
  facing,
  visible,
}: {
  armed: boolean;
  visible: boolean;
  shape: GuiSurfaceShape;
  facing: SurfaceFacing;
}) {
  const pose = shieldPose(shape, facing, REST_LAYER_SPACING);
  const size = [
    pose.sx * PANEL_SCALE,
    pose.sy * PANEL_SCALE,
    pose.sz * PANEL_SCALE,
  ] as const;
  return (
    <Entity id="gui-input-shield-frame">
      {/* Geometry changes author the base frame; only its neutral child
          receives Host animation contributions. */}
      <Transform {...pose} />
      <Children>
        <Entity id={SHIELD_ENTITY}>
          <Transform />
          <PickingGeometry geometry={encodeBoundingShape(SHIELD_PICKING)} />
          <MeshInstance source={SHIELD_MESH} />
          <BoundingGeometry />
          <CustomMaterial
            source={assetRef("gui-input-shield-shader")}
            size={size}
            // Armed: amber frame and hatching. Lifted: a plain grey frame, still
            // in front of the button.
            color={armed ? [1, 0.62, 0.12, 1] : [0.38, 0.42, 0.46, 1]}
            hatch={armed ? 1 : 0}
            visible={visible ? 1 : 0}
            // Cutout marks write depth, so the translucent panel drawn behind
            // them never washes them out; the glass between them is cut away.
            alpha_mode={1}
            receives_light={false}
            receives_shadows={false}
            casts_shadows={false}
          />
        </Entity>
      </Children>
    </Entity>
  );
}
