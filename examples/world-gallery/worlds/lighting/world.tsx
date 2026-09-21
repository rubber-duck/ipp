import { clientAssetSource } from "@ipp/client";
import type { BoundingShape, GeometryEncoder } from "@ipp/client";
import {
  BoundingGeometry,
  Entity,
  Light,
  MeshInstance,
  PbrMaterial,
  PickingGeometry,
  Transform,
  UnlitMaterial,
} from "@ipp/react";
import { World, useIppCanvas } from "@ipp/react/web";
import { hexToLinear } from "../../shared/colors.js";
import { ReadyGeometry } from "../../shared/ready-geometry.js";
import { GALLERY_RUNTIME } from "../../shared/runtime.js";
import {
  initialMeshSettings,
  meshSource,
  type GeometryParameters,
} from "../../shared/geometry-catalog.js";
import { BEAM_ASSET, beamPickingSource } from "./animation-assets.js";
import type {
  ObjectId,
  ObjectSettings,
  LightingWorldObjects,
} from "./model.js";

const { encodeBoundingShape }: { encodeBoundingShape: GeometryEncoder } =
  await import(`${GALLERY_RUNTIME}generated.js`);

/** Compose the demonstrations; each object owns its material and picking geometry. */
export function LightingWorld({
  objects,
  selected,
}: {
  objects: LightingWorldObjects;
  selected: ObjectId | undefined;
}) {
  return (
    <World>
      <Entity id="lighting-floor">
        <Transform y={-0.12} />
        <MeshInstance source="ipp://mesh/cube?width=8&height=0.24&length=7" />
        <BoundingGeometry />
        <PbrMaterial
          r={0.42}
          g={0.46}
          b={0.53}
          roughness={0.85}
          cast_shadows={false}
        />
      </Entity>
      <SolidObject
        id="lighting-cube"
        shape="cube"
        value={objects["lighting-cube"]}
        selected={selected === "lighting-cube"}
      />
      <SolidObject
        id="lighting-sphere"
        shape="sphere"
        value={objects["lighting-sphere"]}
        selected={selected === "lighting-sphere"}
      />
      <SolidObject
        id="lighting-pillar"
        shape="pill"
        value={objects["lighting-pillar"]}
        selected={selected === "lighting-pillar"}
      />
      <LightMarker
        id="lighting-spot"
        value={objects["lighting-spot"]}
        selected={selected === "lighting-spot"}
      />
      <LightMarker
        id="lighting-point"
        value={objects["lighting-point"]}
        selected={selected === "lighting-point"}
      />
      <LightMarker
        id="lighting-fill"
        value={objects["lighting-fill"]}
        selected={selected === "lighting-fill"}
      />
      <SkinnedBeam
        value={objects["lighting-skinning"]}
        selected={selected === "lighting-skinning"}
      />
    </World>
  );
}

type SolidShape = "cube" | "sphere" | "pill";

/** A PBR mesh with conservative culling bounds and a matching pick primitive. */
function SolidObject({
  id,
  shape,
  value,
  selected,
}: {
  id: string;
  shape: SolidShape;
  value: ObjectSettings;
  selected: boolean;
}) {
  const source = meshSource(shape, {
    ...initialMeshSettings(shape),
    parameters: value.parameters,
  });
  const [r, g, b] = hexToLinear(value.color);
  return (
    <ReadyGeometry id={`${id}-pending`} mesh={source} texture={undefined}>
      {({ mesh }) => (
        <Entity id={id}>
          <Transform
            x={value.position[0]}
            y={value.position[1]}
            z={value.position[2]}
            sx={value.scale}
            sy={value.scale}
            sz={value.scale}
            {...(id === "lighting-cube"
              ? { qy: Math.sin(0.15), qw: Math.cos(0.15) }
              : {})}
          />
          <MeshInstance source={mesh} />
          <PbrMaterial
            r={r}
            g={g}
            b={b}
            roughness={value.roughness}
            metallic={value.metallic}
            cast_shadows={value.castShadows}
            receive_shadows={value.receiveShadows}
          />
          <BoundingGeometry />
          <PickingGeometry
            geometry={encodeBoundingShape(
              pickingShape(shape, mesh, value.parameters),
            )}
            outline
            is_rendered={selected}
            stroke={0.025}
            color={[1, 0.8, 0]}
          />
        </Entity>
      )}
    </ReadyGeometry>
  );
}

function pickingShape(
  shape: SolidShape,
  source: string,
  fallback: GeometryParameters,
): BoundingShape {
  // Use the displayed mesh's dimensions while a replacement is still loading.
  const query = new URL(source).searchParams;
  const dimension = (name: keyof GeometryParameters) =>
    query.has(name) ? Number(query.get(name)) : fallback[name];
  if (shape === "sphere")
    return { type: "sphere", radius: dimension("radius") };
  if (shape === "pill") {
    const radius = dimension("radius");
    const halfLength = dimension("height") / 2 - radius;
    return {
      type: "pill",
      radius,
      start: [0, -halfLength, 0],
      end: [0, halfLength, 0],
    };
  }
  const x = dimension("width") / 2;
  const y = dimension("height") / 2;
  const z = dimension("length") / 2;
  return { type: "box", min: [-x, -y, -z], max: [x, y, z] };
}

/** A light and its visible marker share one transform, including animated motion. */
function LightMarker({
  id,
  value,
  selected,
}: {
  id: "lighting-spot" | "lighting-point" | "lighting-fill";
  value: ObjectSettings;
  selected: boolean;
}) {
  const source =
    id === "lighting-point"
      ? "ipp://mesh/sphere?radius=0.16"
      : new URL(
          `/target/gallery-build/${id === "lighting-spot" ? "spot" : "sun"}-marker.mesh`,
          window.location.href,
        ).href;
  const [r, g, b] = hexToLinear(value.color);
  const scale = value.scale * value.markerSize;
  const pose = id === "lighting-point" ? {} : aimAt(...value.position);
  return (
    <ReadyGeometry id={`${id}-pending`} mesh={source} texture={undefined}>
      {({ mesh }) => (
        <Entity id={id}>
          <Transform
            x={value.position[0]}
            y={value.position[1]}
            z={value.position[2]}
            sx={scale}
            sy={scale}
            sz={scale}
            {...pose}
          />
          <MeshInstance source={mesh} />
          <UnlitMaterial r={r} g={g} b={b} />
          <Light
            kind={id === "lighting-spot" ? 2 : id === "lighting-point" ? 1 : 0}
            r={r}
            g={g}
            b={b}
            intensity={value.intensity}
            range={value.range}
            inner_cone={value.innerCone}
            outer_cone={value.outerCone}
            cast_shadows={id === "lighting-spot" && value.castShadows}
          />
          <BoundingGeometry />
          {/* Mesh-derived boxes preserve the cone apex and arrow plane at the +Z face. */}
          <PickingGeometry
            outline
            is_rendered={selected}
            stroke={0.025}
            color={[1, 0.8, 0]}
          />
        </Entity>
      )}
    </ReadyGeometry>
  );
}

/** Orient local -Z toward a point; markers follow the same convention as lights. */
function aimAt(x: number, y: number, z: number, targetY = 0) {
  const yaw = Math.atan2(x, z) / 2;
  const pitch = -Math.atan2(y - targetY, Math.hypot(x, z)) / 2;
  return {
    qx: Math.sin(pitch) * Math.cos(yaw),
    qy: Math.cos(pitch) * Math.sin(yaw),
    qz: -Math.sin(pitch) * Math.sin(yaw),
    qw: Math.cos(pitch) * Math.cos(yaw),
  };
}

/** The playback session attaches the skeleton and skin after this entity exists. */
function SkinnedBeam({
  value,
  selected,
}: {
  value: ObjectSettings;
  selected: boolean;
}) {
  const canvas = useIppCanvas();
  if (!canvas) return null;
  const [r, g, b] = hexToLinear(value.color);
  return (
    <ReadyGeometry
      id="lighting-skinning-pending"
      mesh={clientAssetSource(canvas.client.session, 1, BEAM_ASSET).source}
      texture={undefined}
    >
      {({ mesh }) => (
        <Entity id="lighting-skinning">
          <Transform
            x={value.position[0]}
            y={value.position[1]}
            z={value.position[2]}
            sx={(value.scale * value.parameters.width) / 0.5}
            sy={(value.scale * value.parameters.height) / 2}
            sz={(value.scale * value.parameters.length) / 0.5}
          />
          <MeshInstance source={mesh} />
          <PbrMaterial
            r={r}
            g={g}
            b={b}
            roughness={value.roughness}
            metallic={value.metallic}
            cast_shadows={value.castShadows}
            receive_shadows={value.receiveShadows}
          />
          {/* Culling encloses the deformed mesh; picking follows the two bone pills. */}
          <BoundingGeometry />
          <PickingGeometry
            source={beamPickingSource(canvas.client.session)}
            outline
            is_rendered={selected}
            stroke={0.025}
            color={[1, 0.8, 0]}
          />
        </Entity>
      )}
    </ReadyGeometry>
  );
}
