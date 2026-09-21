import type { ClientAssetSource } from "@ipp/client";
import {
  Animation,
  BoundingGeometry,
  CustomMaterial,
  Entity,
  Light,
  MeshInstance,
  PbrMaterial,
  Transform,
  UnlitMaterial,
  UnlitTexture,
  assetRef,
  texture2D,
  type AnimationHandle,
} from "@ipp/react";
import { useEffect, useRef } from "react";
import type { GuiDemoSkin, GuiSceneState } from "./scene.js";

type Color = readonly [number, number, number];
type Point = readonly [number, number, number];

const ASSET_ROOT = "/target/gallery-gui-assets/projector/";
const BACKGROUND_MESH = "ipp://mesh/cube?width=1&height=1&length=1";
const PROJECTOR_CENTER: Point = [0, 0.3, 0];
const BASE_CENTER: Point = [0, 0, 0];
const PROJECTOR_SCALE = 2.2;
const PANEL_DISTANCE = 5.1;
export const PANEL_SCALE = 0.7;

// One rigid orientation owns the emitting face, beam, and Surface. Its local
// +Z axis points from the recessed projector lens to the panel center.
const PROJECTOR_YAW = 0.3;
const PROJECTOR_PITCH = -0.04;
export const PROJECTOR_ROTATION = {
  qx: Math.sin(PROJECTOR_PITCH / 2) * Math.cos(PROJECTOR_YAW / 2),
  qy: Math.cos(PROJECTOR_PITCH / 2) * Math.sin(PROJECTOR_YAW / 2),
  qz: -Math.sin(PROJECTOR_PITCH / 2) * Math.sin(PROJECTOR_YAW / 2),
  qw: Math.cos(PROJECTOR_PITCH / 2) * Math.cos(PROJECTOR_YAW / 2),
};

function rotatedAxis(axis: Point): Point {
  const { qx, qy, qz, qw } = PROJECTOR_ROTATION;
  const [x, y, z] = axis;
  return [
    (1 - 2 * (qy * qy + qz * qz)) * x +
      2 * (qx * qy - qw * qz) * y +
      2 * (qx * qz + qw * qy) * z,
    2 * (qx * qy + qw * qz) * x +
      (1 - 2 * (qx * qx + qz * qz)) * y +
      2 * (qy * qz - qw * qx) * z,
    2 * (qx * qz - qw * qy) * x +
      2 * (qy * qz + qw * qx) * y +
      (1 - 2 * (qx * qx + qy * qy)) * z,
  ];
}

const PROJECTOR_AXIS = rotatedAxis([0, 0, 1]);
const PANEL_CENTER: Point = PROJECTOR_CENTER.map(
  (value, axis) => value + PROJECTOR_AXIS[axis]! * PANEL_DISTANCE,
) as [number, number, number];
const ACCENTS: Readonly<Record<GuiDemoSkin, Color>> = {
  aurora: [0.18, 0.86, 1],
  ember: [1, 0.34, 0.08],
};

export const PROJECTOR_MESH_SOURCES = [
  `${ASSET_ROOT}shell.ippm`,
  `${ASSET_ROOT}trim.ippm`,
  `${ASSET_ROOT}aperture.ippm`,
  `${ASSET_ROOT}lens.ippm`,
  `${ASSET_ROOT}frustum.ippm`,
  `${ASSET_ROOT}base.ippm`,
  `${ASSET_ROOT}floor.ippm`,
] as const;

export const PROJECTOR_TEXTURE_SOURCES = [
  `${ASSET_ROOT}metal-base.ippt`,
  `${ASSET_ROOT}base-baked.ippt`,
  `${ASSET_ROOT}floor-baked.ippt`,
] as const;

export interface ProjectorAssets {
  readonly shell: string;
  readonly trim: string;
  readonly aperture: string;
  readonly lens: string;
  readonly beam: string;
  readonly base: string;
  readonly floor: string;
  readonly metal: string;
  readonly baseBaked: string;
  readonly floorBaked: string;
}

export function projectorAssets(): ProjectorAssets {
  const absolute = (path: string) =>
    new URL(path, globalThis.location.href).href;
  return {
    shell: absolute(PROJECTOR_MESH_SOURCES[0]),
    trim: absolute(PROJECTOR_MESH_SOURCES[1]),
    aperture: absolute(PROJECTOR_MESH_SOURCES[2]),
    lens: absolute(PROJECTOR_MESH_SOURCES[3]),
    beam: absolute(PROJECTOR_MESH_SOURCES[4]),
    base: absolute(PROJECTOR_MESH_SOURCES[5]),
    floor: absolute(PROJECTOR_MESH_SOURCES[6]),
    metal: absolute(PROJECTOR_TEXTURE_SOURCES[0]),
    baseBaked: absolute(PROJECTOR_TEXTURE_SOURCES[1]),
    floorBaked: absolute(PROJECTOR_TEXTURE_SOURCES[2]),
  };
}

export function projectorResourceSources(): readonly ClientAssetSource[] {
  const assets = projectorAssets();
  return [
    { kind: 1, source: BACKGROUND_MESH },
    ...[
      assets.shell,
      assets.trim,
      assets.aperture,
      assets.lens,
      assets.beam,
      assets.base,
      assets.floor,
    ].map((source) => ({ kind: 1, source })),
    ...[assets.metal, assets.baseBaked, assets.floorBaked].map((source) => ({
      kind: 2,
      source,
    })),
  ];
}

export interface ProjectorBeamSection {
  readonly halfSize: readonly [number, number, number, number];
  readonly depth: readonly [number, number];
}

/** The authored mesh manifest owns the near/far cross-sections used by its shader. */
export async function loadProjectorBeamSection(
  signal: AbortSignal,
): Promise<ProjectorBeamSection> {
  const response = await fetch(`${ASSET_ROOT}projector.json`, { signal });
  if (!response.ok)
    throw new Error(`Projector metadata failed: ${response.status}`);
  const metadata = (await response.json()) as {
    projection: {
      nearHalfSize: [number, number];
      farHalfSize: [number, number];
      nearZ: number;
      farZ: number;
    };
  };
  const section = [
    ...metadata.projection.nearHalfSize,
    ...metadata.projection.farHalfSize,
  ];
  if (
    section.length !== 4 ||
    !section.every((value) => Number.isFinite(value) && value > 0)
  ) {
    throw new Error("Projector metadata has an invalid beam cross-section");
  }
  const depth = [metadata.projection.nearZ, metadata.projection.farZ] as const;
  if (!depth.every(Number.isFinite) || depth[0] < 0 || depth[1] <= depth[0])
    throw new Error("Projector metadata has an invalid beam depth");
  return { halfSize: section as [number, number, number, number], depth };
}

function scaled(color: Color, amount: number): Color {
  return [
    Math.min(1, color[0] * amount),
    Math.min(1, color[1] * amount),
    Math.min(1, color[2] * amount),
  ];
}

function placed(point: Point, stagingX: number) {
  return {
    x: stagingX + point[0],
    y: point[1],
    z: point[2],
    ...PROJECTOR_ROTATION,
  };
}

function basePlaced(stagingX: number) {
  return {
    x: stagingX + BASE_CENTER[0],
    y: BASE_CENTER[1],
    z: BASE_CENTER[2],
    qy: Math.sin(PROJECTOR_YAW / 2),
    qw: Math.cos(PROJECTOR_YAW / 2),
  };
}

/** Authored projector parts and Host animation compose around the real GUI panel. */
export function HolographicProjector({
  scene,
  stagingX,
}: {
  scene: GuiSceneState;
  stagingX: number;
}) {
  const dust = useRef<AnimationHandle>(null);
  const accent = ACCENTS[scene.skin];
  const energy = 0.38 + scene.gain * 0.62;
  const energized = scaled(accent, energy);
  const assets = projectorAssets();

  useEffect(() => {
    if (!scene.revealed) return;
    let active = true;
    void dust.current?.play().catch((failure: unknown) => {
      if (active) scene.reportFailure(failure);
    });
    return () => {
      active = false;
    };
  }, [scene.revealed, scene.reportFailure]);

  return (
    <>
      <Entity id="gui-projector-background">
        <Transform x={stagingX} />
        <MeshInstance source={BACKGROUND_MESH} />
        <BoundingGeometry />
        <CustomMaterial
          source={assetRef("gui-projector-background-shader")}
          visible={scene.revealed ? 1 : 0}
          receives_light={false}
          receives_shadows={false}
          casts_shadows={false}
        />
      </Entity>
      <Entity id="gui-projector-floor">
        <Transform {...basePlaced(stagingX)} sx={1.55} sy={1.55} sz={1.55} />
        <MeshInstance source={assets.floor} />
        <BoundingGeometry />
        <UnlitTexture source={assets.floorBaked} />
        <UnlitMaterial r={1} g={1} b={1} />
      </Entity>

      <Entity id="gui-projector-base">
        <Transform {...basePlaced(stagingX)} sx={1.55} sy={1.55} sz={1.55} />
        <MeshInstance source={assets.base} />
        <BoundingGeometry />
        <UnlitTexture source={assets.baseBaked} />
        <UnlitMaterial r={1} g={1} b={1} />
      </Entity>

      <Entity id="gui-projector-core">
        <Transform
          {...placed(PROJECTOR_CENTER, stagingX)}
          sx={PROJECTOR_SCALE}
          sy={PROJECTOR_SCALE}
          sz={PROJECTOR_SCALE}
        />
        <MeshInstance source={assets.shell} />
        <BoundingGeometry />
        <CustomMaterial
          source={assetRef("gui-projector-metal")}
          base={texture2D(assets.metal)}
          accent={[accent[0], accent[1], accent[2], 1]}
          energy={energy}
          receives_light
          receives_shadows={false}
          casts_shadows={false}
        />
      </Entity>

      <Entity id="gui-projector-trim">
        <Transform
          {...placed(PROJECTOR_CENTER, stagingX)}
          sx={PROJECTOR_SCALE}
          sy={PROJECTOR_SCALE}
          sz={PROJECTOR_SCALE}
        />
        <MeshInstance source={assets.trim} />
        <BoundingGeometry />
        <PbrMaterial
          r={0.27}
          g={0.34}
          b={0.38}
          metallic={0.85}
          roughness={0.22}
          receive_shadows={false}
          cast_shadows={false}
        />
      </Entity>

      <Entity id="gui-projector-aperture">
        <Transform
          {...placed(PROJECTOR_CENTER, stagingX)}
          sx={PROJECTOR_SCALE}
          sy={PROJECTOR_SCALE}
          sz={PROJECTOR_SCALE}
        />
        <MeshInstance source={assets.aperture} />
        <BoundingGeometry />
        <PbrMaterial
          r={0.006}
          g={0.014}
          b={0.022}
          metallic={0.78}
          roughness={0.22}
          cast_shadows={false}
          receive_shadows={false}
        />
      </Entity>

      <Entity id="gui-projector-emitter">
        <Transform
          {...placed(PROJECTOR_CENTER, stagingX)}
          sx={PROJECTOR_SCALE}
          sy={PROJECTOR_SCALE}
          sz={PROJECTOR_SCALE}
        />
        <MeshInstance source={assets.lens} />
        <BoundingGeometry />
        <CustomMaterial
          source={assetRef("gui-projector-glow")}
          accent={[accent[0], accent[1], accent[2], 1]}
          energy={0.75 + scene.gain * 0.8}
          alpha_mode={2}
          receives_light
          receives_shadows={false}
          casts_shadows={false}
        />
      </Entity>

      <Entity id="gui-projector-beam">
        <Transform {...placed(PROJECTOR_CENTER, stagingX)} />
        <MeshInstance source={assets.beam} />
        <BoundingGeometry />
        <CustomMaterial
          source={assetRef("gui-projector-beam-shader")}
          section={scene.beamSection!.halfSize}
          depth={scene.beamSection!.depth}
          phase={0}
          accent={[accent[0], accent[1], accent[2], 1]}
          energy={energy}
          alpha_mode={2}
          receives_light
          receives_shadows={false}
          casts_shadows={false}
        />
        <Animation
          ref={dust}
          source={scene.motions!.dust.source}
          bindings={[
            {
              track: 0,
              ...(scene.motions!.dust.variant === undefined
                ? {}
                : { variant: scene.motions!.dust.variant }),
              property: {
                component: scene.motions!.materialComponent,
                name: "phase",
              },
            },
          ]}
          looping
          autoPlay={false}
        />
      </Entity>

      <Entity id="gui-projector-light">
        <Transform
          {...placed(
            PROJECTOR_CENTER.map(
              (value, axis) => value + PROJECTOR_AXIS[axis]! * 1.25,
            ) as [number, number, number],
            stagingX,
          )}
        />
        <Light
          kind={1}
          r={energized[0]}
          g={energized[1]}
          b={energized[2]}
          intensity={0.4 + scene.gain * 1.8}
          range={7.5}
          cast_shadows={false}
        />
      </Entity>

      <Entity id="gui-projector-key-light">
        <Transform x={stagingX - 3.5} y={6} z={4.5} />
        <Light
          kind={1}
          r={0.9}
          g={0.95}
          b={1}
          intensity={80}
          range={20}
          cast_shadows={false}
        />
      </Entity>
      <Entity id="gui-projector-fill-light">
        <Transform x={stagingX - 5} y={2} z={9} />
        <Light
          kind={1}
          r={0.8}
          g={0.88}
          b={1}
          intensity={32}
          range={24}
          cast_shadows={false}
        />
      </Entity>
    </>
  );
}

export const PROJECTED_PANEL_TRANSFORM = {
  x: PANEL_CENTER[0],
  y: PANEL_CENTER[1],
  z: PANEL_CENTER[2],
  ...PROJECTOR_ROTATION,
  sx: PANEL_SCALE,
  sy: PANEL_SCALE,
  sz: PANEL_SCALE,
} as const;

export type ProjectorMotionAssets = {
  readonly dust: ClientAssetSource;
  readonly materialComponent: number;
};
