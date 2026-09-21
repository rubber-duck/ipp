import type { AnimationKeyframe, AnimationJointTransform } from "@ipp/client";

/** Detached authoring v1. Sources resolve against the authenticated addon origin. */
export interface BlenderSnapshot {
  type: "snapshot";
  session: string;
  revision: number;
  scene: BlenderScene;
}

export interface BlenderScene {
  /** Linear World Background RGB multiplied by its strength. */
  ambient_light?: [number, number, number];
  entities: BlenderEntity[];
  active_camera?: string;
  animations?: BlenderAnimation[];
  /** Reusable action/slot associations, independent of active playback. */
  clips?: { id: string; name: string; target: string; source: string }[];
  diagnostics?: { code: string; message: string; entity?: string }[];
}

export interface BlenderEntity {
  id: string;
  name?: string;
  /** Stable producer identity; Transform is local to this parent. */
  parent?: string;
  /** Optional zero-based joint index in the parent Skeleton. */
  parent_bone?: number;
  transform?: {
    x: number;
    y: number;
    z: number;
    qx: number;
    qy: number;
    qz: number;
    qw: number;
    sx: number;
    sy: number;
    sz: number;
  };
  particle_emitter?: Record<string, number | boolean | string>;
  particle_playback?: { source: string; time: number };
  particle_sprite?: Record<string, number | string>;
  particle_mesh?: { source: string };
  mesh?: { source: string };
  mesh_pose?: { source: string; weight: number };
  texture?: { source: string };
  material?: {
    type: "unlit" | "pbr";
    r: number;
    g: number;
    b: number;
    metallic?: number;
    roughness?: number;
    cast_shadows?: boolean;
    receive_shadows?: boolean;
  };
  camera?: {
    projection: 0 | 1;
    fov_y: number;
    near: number;
    far: number;
    ortho_height: number;
    focus_distance: number;
  };
  light?: {
    kind: 0 | 1 | 2;
    r: number;
    g: number;
    b: number;
    intensity: number;
    range: number;
    inner_cone: number;
    outer_cone: number;
    cast_shadows?: boolean;
    shadow_near?: number;
    shadow_bias?: number;
    shadow_radius?: number;
  };
  skeleton?: { source: string; pose_source?: string };
  skin?: { source: string; skeleton: string };
}

export interface BlenderAnimation {
  id: string;
  target: string;
  /** HTTPS JSON BlenderClip, encoded with the receiving runtime's generated SDK. */
  source: string;
  looping?: boolean;
  speed?: number;
  autoplay?: boolean;
}

export interface BlenderClip {
  duration: number;
  tracks: (
    | {
        property: { component: string; fields: string[] };
        keys: AnimationKeyframe[];
      }
    | { joints: number[]; keys: AnimationKeyframe[] }
  )[];
}

export type BlenderJointTransform = AnimationJointTransform;
