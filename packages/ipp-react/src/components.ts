/** Static world components; contract resolution belongs to the receiving root. */
import { createElement } from "react";
import type { DynamicPropertyInput } from "@ipp/client";
import type { SurfaceContent, SurfaceStyle } from "@ipp/client";
import type { AssetReference } from "./assets.js";
import type { ReactNode } from "react";

export const ENTITY_HOST_TYPE = "ipp-entity";
export const CHILDREN_HOST_TYPE = "ipp-children";
/** One static authoring manifest drives prop types and host-to-contract lookup. */
export const componentContract = {
  Surface: {
    host: "ipp-surface",
    fields: { width: "number", height: "number", items: "bytes" },
  },
  SurfaceCache: {
    host: "ipp-surface-cache",
    fields: {
      direct_distance: "number",
      texels_per_metre: "number",
      max_refresh_hz: "number",
    },
  },
  ParticleEmitter: {
    host: "ipp-particle-emitter",
    fields: {
      enabled: "boolean",
      seed: "number",
      restart: "number",
      capacity: "number",
      rate: "number",
      duration: "number",
      delay: "number",
      burst: "number",
      space: "number",
      shape: "number",
      source: "string",
      variant: "number",
      extent_x: "number",
      extent_y: "number",
      extent_z: "number",
      lifetime: "number",
      lifetime_random: "number",
      speed: "number",
      speed_random: "number",
      spread: "number",
      size: "number",
      size_random: "number",
      rotation_random: "number",
      spin: "number",
      acceleration_x: "number",
      acceleration_y: "number",
      acceleration_z: "number",
      drag: "number",
    },
  },
  ParticlePlayback: {
    host: "ipp-particle-playback",
    fields: { source: "string", variant: "number", time: "number" },
  },
  ParticleSprite: {
    host: "ipp-particle-sprite",
    fields: {
      source: "string",
      variant: "number",
      r: "number",
      g: "number",
      b: "number",
      opacity: "number",
      end_opacity: "number",
      end_size: "number",
      blend: "number",
      alignment: "number",
    },
  },
  ParticleMesh: {
    host: "ipp-particle-mesh",
    fields: { source: "string", variant: "number" },
  },

  Hierarchy: {
    host: "ipp-hierarchy",
    fields: { parent: "bigint", parent_bone: "number" },
  },
  LookAt: {
    host: "ipp-look-at",
    fields: { target: "bigint", enabled: "boolean" },
  },
  Scalar: { host: "ipp-scalar", fields: { value: "number" } },
  Transform: {
    host: "ipp-transform",
    fields: {
      x: "number",
      y: "number",
      z: "number",
      qx: "number",
      qy: "number",
      qz: "number",
      qw: "number",
      sx: "number",
      sy: "number",
      sz: "number",
    },
  },
  UnlitMaterial: {
    host: "ipp-unlit-material",
    fields: { r: "number", g: "number", b: "number" },
  },
  CustomMaterial: {
    host: "ipp-custom-material",
    fields: {
      source: "string",
      variant: "number",
      alpha_mode: "number",
      alpha_cutoff: "number",
      receives_light: "boolean",
      receives_shadows: "boolean",
      casts_shadows: "boolean",
      conservative_bounds: "boolean",
    },
  },
  PbrMaterial: {
    host: "ipp-pbr-material",
    fields: {
      r: "number",
      g: "number",
      b: "number",
      metallic: "number",
      roughness: "number",
      cast_shadows: "boolean",
      receive_shadows: "boolean",
    },
  },
  Light: {
    host: "ipp-light",
    fields: {
      kind: "number",
      r: "number",
      g: "number",
      b: "number",
      intensity: "number",
      range: "number",
      inner_cone: "number",
      outer_cone: "number",
      cast_shadows: "boolean",
      shadow_near: "number",
      shadow_bias: "number",
      shadow_radius: "number",
    },
  },
  MeshInstance: {
    host: "ipp-mesh-instance",
    fields: { source: "string", variant: "number" },
  },
  UnlitTexture: {
    host: "ipp-unlit-texture",
    fields: { source: "string", variant: "number" },
  },
  Camera: {
    host: "ipp-camera",
    fields: {
      projection: "number",
      fov_y: "number",
      near: "number",
      far: "number",
      ortho_height: "number",
      focus_distance: "number",
    },
  },
  BoundingGeometry: {
    host: "ipp-bounding-geometry",
    fields: {
      geometry: "bytes",
      source: "string",
      variant: "number",
      skeleton: "bigint",
      is_rendered: "boolean",
      outline: "boolean",
      stroke: "number",
      has_color_override: "boolean",
      r: "number",
      g: "number",
      b: "number",
    },
  },
  PickingGeometry: {
    host: "ipp-picking-geometry",
    fields: {
      geometry: "bytes",
      source: "string",
      variant: "number",
      skeleton: "bigint",
      is_rendered: "boolean",
      outline: "boolean",
      stroke: "number",
      has_color_override: "boolean",
      r: "number",
      g: "number",
      b: "number",
    },
  },
} as const;

type ComponentName = keyof typeof componentContract;
export type ReactWorldComponentType =
  (typeof componentContract)[ComponentName]["host"];
export const componentNames = Object.fromEntries(
  Object.entries(componentContract).map(([name, definition]) => [
    definition.host,
    name,
  ]),
) as Readonly<Record<ReactWorldComponentType, ComponentName>>;

type Primitive<T> = T extends "number"
  ? number
  : T extends "boolean"
    ? boolean
    : T extends "bigint"
      ? bigint
      : T extends "string"
        ? string
        : T extends "bytes"
          ? Uint8Array<ArrayBuffer>
          : never;
type ComponentFields<Name extends ComponentName> = {
  [Field in keyof (typeof componentContract)[Name]["fields"]]?:
    | (Field extends "source"
        ? string | AssetReference
        : Primitive<(typeof componentContract)[Name]["fields"][Field]>)
    | undefined;
};

export type EntityProps = { children?: ReactNode } & (
  | { id: string; bindTo?: never }
  | { id?: never; bindTo: string }
);

export interface ComponentProps {
  children?: ReactNode;
  bound?: boolean | null | undefined;
}

/** A key retains the same item identity through edits and painter-order changes. */
export interface SurfaceItemProps extends SurfaceStyle {
  key: string;
  content: SurfaceContent;
}

export type SurfaceProps = ComponentProps &
  Omit<ComponentFields<"Surface">, "items"> & {
    /** Keyed collections require an owned component (`bound={false}`). */
    items?: readonly SurfaceItemProps[] | Uint8Array<ArrayBuffer>;
    [name: string]: unknown;
  };

export function Surface(props: SurfaceProps) {
  return createElement(componentContract.Surface.host, props);
}

/**
 * Opt the Surface on the same entity into whole-Surface texture caching.
 *
 * Without this component the Surface always presents directly. Cached
 * presentation is a renderer optimization for distant Surfaces: nearby
 * Surfaces and GUI roots with focus, hover, press or capture still present
 * directly, and World, input and animation updates are never throttled.
 * Omitted props use the runtime defaults; the runtime rejects values outside
 * the documented ranges without changing other state. Only these authored
 * values persist in World snapshots; cached images and schedules do not.
 */
export type SurfaceCacheProps = ComponentProps & {
  /**
   * Camera-to-Surface distance in metres below which the Surface presents
   * directly. Farther distances select cached bands that halve resolution
   * and refresh rate each time the distance doubles. `0` caches at every
   * distance. Finite and non-negative.
   */
  direct_distance?: number | undefined;
  /** Cache texel density per Surface metre in the nearest cached band; positive. */
  texels_per_metre?: number | undefined;
  /**
   * Maximum content refresh rate in hertz in the nearest cached band;
   * positive. Pending content changes wait for the next refresh.
   */
  max_refresh_hz?: number | undefined;
};

export function SurfaceCache(props: SurfaceCacheProps) {
  return createElement(componentContract.SurfaceCache.host, props);
}

export interface ChildrenProps {
  children?: ReactNode;
}

export type HierarchyProps = ComponentProps & ComponentFields<"Hierarchy">;
export type LookAtProps = ComponentProps & ComponentFields<"LookAt">;

export type ScalarProps = ComponentProps & ComponentFields<"Scalar">;

export type TransformProps = ComponentProps &
  ComponentFields<"Transform"> & {
    /** Euler angles in radians, intrinsic XYZ order. Cannot be mixed with q props. */
    rx?: number | undefined;
    ry?: number | undefined;
    rz?: number | undefined;
  };

export type UnlitMaterialProps = ComponentProps &
  ComponentFields<"UnlitMaterial">;
export type MeshInstanceProps = ComponentProps &
  ComponentFields<"MeshInstance">;
export type UnlitTextureProps = ComponentProps &
  ComponentFields<"UnlitTexture">;
export type CameraProps = ComponentProps & ComponentFields<"Camera">;
export type BoundingGeometryProps = ComponentProps &
  Omit<
    ComponentFields<"BoundingGeometry">,
    "has_color_override" | "r" | "g" | "b"
  > & {
    /** Uniform linear RGB; omission follows the global geometry color. */
    color?: readonly [number, number, number] | undefined;
  };
export type PickingGeometryProps = BoundingGeometryProps;

export function Entity(props: EntityProps) {
  return createElement<EntityProps>(ENTITY_HOST_TYPE, props);
}

/** Mount entities with an Auto Hierarchy pointing to the enclosing Entity. */
export function Children(props: ChildrenProps) {
  return createElement(CHILDREN_HOST_TYPE, props);
}

export function Hierarchy(props: HierarchyProps) {
  return createElement(componentContract.Hierarchy.host, props);
}

export function LookAt(props: LookAtProps) {
  return createElement(componentContract.LookAt.host, props);
}

export function Scalar(props: ScalarProps) {
  return createElement(componentContract.Scalar.host, props);
}

export function Transform(props: TransformProps) {
  const { rx, ry, rz, ...fields } = props;
  if (rx === undefined && ry === undefined && rz === undefined) {
    return createElement(componentContract.Transform.host, fields);
  }
  if (
    [fields.qx, fields.qy, fields.qz, fields.qw].some((q) => q !== undefined)
  ) {
    throw new Error("Transform cannot mix rx/ry/rz with qx/qy/qz/qw");
  }
  if ([rx, ry, rz].some((r) => r !== undefined && !Number.isFinite(r))) {
    throw new RangeError("Transform Euler angles must be finite radians");
  }

  const x = (rx ?? 0) / 2;
  const y = (ry ?? 0) / 2;
  const z = (rz ?? 0) / 2;
  const cx = Math.cos(x);
  const sx = Math.sin(x);
  const cy = Math.cos(y);
  const sy = Math.sin(y);
  const cz = Math.cos(z);
  const sz = Math.sin(z);

  // Intrinsic XYZ: qX * qY * qZ. Euler input supplies the whole rotation.
  return createElement(componentContract.Transform.host, {
    ...fields,
    qx: sx * cy * cz + cx * sy * sz,
    qy: cx * sy * cz - sx * cy * sz,
    qz: cx * cy * sz + sx * sy * cz,
    qw: cx * cy * cz - sx * sy * sz,
  });
}

export function UnlitMaterial(props: UnlitMaterialProps) {
  return createElement(componentContract.UnlitMaterial.host, props);
}

export function MeshInstance(props: MeshInstanceProps) {
  return createElement(componentContract.MeshInstance.host, props);
}

export function UnlitTexture(props: UnlitTextureProps) {
  return createElement(componentContract.UnlitTexture.host, props);
}

export function Camera(props: CameraProps) {
  return createElement(componentContract.Camera.host, props);
}

export function BoundingGeometry(props: BoundingGeometryProps) {
  return geometryElement(componentContract.BoundingGeometry.host, props);
}

export function PickingGeometry(props: PickingGeometryProps) {
  return geometryElement(componentContract.PickingGeometry.host, props);
}

function geometryElement(
  host: string,
  { color, ...props }: BoundingGeometryProps,
) {
  return createElement(host, {
    ...props,
    has_color_override: color === undefined ? undefined : true,
    r: color?.[0],
    g: color?.[1],
    b: color?.[2],
  });
}

export type PbrMaterialProps = ComponentProps & ComponentFields<"PbrMaterial">;
export type LightProps = ComponentProps & ComponentFields<"Light">;

export function PbrMaterial(props: PbrMaterialProps) {
  return createElement(componentContract.PbrMaterial.host, props);
}

export function Light(props: LightProps) {
  return createElement(componentContract.Light.host, props);
}

/** Backend-specific immutable shader source with independently editable named parameters. */
export type CustomMaterialProps = ComponentProps &
  ComponentFields<"CustomMaterial"> & {
    children?: ReactNode;
    /** Unknown props are named dynamic values; fixed fields and bound keep their normal meaning. */
    [name: string]: DynamicPropertyInput | ReactNode | AssetReference;
  };

export function CustomMaterial(props: CustomMaterialProps) {
  return createElement(componentContract.CustomMaterial.host, props);
}

export type ParticleEmitterProps = ComponentProps &
  ComponentFields<"ParticleEmitter">;

export function ParticleEmitter(props: ParticleEmitterProps) {
  return createElement(componentContract.ParticleEmitter.host, props);
}

export type ParticlePlaybackProps = ComponentProps &
  ComponentFields<"ParticlePlayback">;

export function ParticlePlayback(props: ParticlePlaybackProps) {
  return createElement(componentContract.ParticlePlayback.host, props);
}

export type ParticleSpriteProps = ComponentProps &
  ComponentFields<"ParticleSprite">;

export function ParticleSprite(props: ParticleSpriteProps) {
  return createElement(componentContract.ParticleSprite.host, props);
}

export type ParticleMeshProps = ComponentProps &
  ComponentFields<"ParticleMesh">;

export function ParticleMesh(props: ParticleMeshProps) {
  return createElement(componentContract.ParticleMesh.host, props);
}
