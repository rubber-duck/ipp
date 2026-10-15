import type { AnimationClipSource, AnimationJointTransform } from "@ipp/client";
import type { PlatformerMode } from "./session.js";

export interface PlatformerPoseOrigins {
  readonly version: 1;
  readonly baseline: readonly AnimationJointTransform[];
  readonly origins: Readonly<
    Record<PlatformerMode, readonly AnimationJointTransform[]>
  >;
  /** Independently sampled Blender poses used by the maintained frame scenario. */
  readonly references: Readonly<
    Record<
      PlatformerMode,
      readonly {
        readonly time: number;
        readonly pose: readonly AnimationJointTransform[];
      }[]
    >
  >;
}

export const POSE_ORIGIN_HOLD_TIME = 1;

/**
 * A gait contributes its change from its first pose, rather than absolute joints.
 * Hold this earlier controller at the gait's first pose relative to the saved
 * baseline. The following gait contribution then reconstructs Blender's pose.
 */
export function poseOriginClip(
  poses: PlatformerPoseOrigins,
  mode: PlatformerMode,
): AnimationClipSource {
  if (
    poses.version !== 1 ||
    poses.baseline.length !== 23 ||
    poses.origins[mode]?.length !== poses.baseline.length
  )
    throw new Error("Platformer pose origins do not match the source rig");
  return {
    duration: 2 * POSE_ORIGIN_HOLD_TIME,
    tracks: [
      {
        joints: poses.baseline.map((_, index) => index),
        keys: [
          { time: 0, value: { kind: "pose", value: poses.baseline } },
          {
            time: POSE_ORIGIN_HOLD_TIME,
            value: { kind: "pose", value: poses.origins[mode] },
          },
          {
            time: 2 * POSE_ORIGIN_HOLD_TIME,
            value: { kind: "pose", value: poses.origins[mode] },
          },
        ],
      },
    ],
  };
}
