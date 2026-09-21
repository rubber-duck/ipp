import {
  Entity,
  Transform,
  UnlitMaterial,
  UnlitTexture,
  MeshInstance,
} from "@ipp/react";
import { World } from "@ipp/react/web";
import {
  MESH_IDS,
  meshSource,
  type MeshSettings,
  type IsolatedShape,
  type ViewerShape,
} from "../../shared/geometry-catalog.js";
import { hexToLinear } from "../../shared/colors.js";
import { ReadyGeometry } from "../../shared/ready-geometry.js";

export const VIEWER_ENTITY_ID = "react-gallery-selection";
export const VIEWER_TEXTURE_SOURCE =
  "ipp://texture/uv-grid?width=512&height=512&cellsX=8&cellsY=8";

/** Props author the world: mesh URI, transform, material override and optional texture. */
export function ShapesWorld({
  shape,
  meshes,
}: {
  readonly shape: ViewerShape;
  readonly meshes: Readonly<Record<IsolatedShape, MeshSettings>>;
}) {
  const overview = shape === "gallery";
  const visible = overview ? MESH_IDS : [shape];
  return (
    <World>
      {visible.map((mesh, index) => {
        const settings = meshes[mesh];
        const id = overview ? `react-gallery-${mesh}` : VIEWER_ENTITY_ID;
        const layoutScale = overview ? 0.36 : 1;
        const scale = layoutScale * settings.scale;
        const x =
          (overview ? ((index % 4) - 1.5) * 1.4 : 0) + settings.x * layoutScale;
        const y = overview ? 1.2 - Math.floor(index / 4) * 1.2 : 0;
        const color = settings.override
          ? hexToLinear(settings.color)
          : undefined;
        // The catalog builds ordinary URIs, e.g. ipp://mesh/cube?width=2&height=2&length=2.
        const source = meshSource(mesh, settings);
        return (
          <ReadyGeometry
            key={id}
            id={`${id}-pending`}
            mesh={source}
            texture={
              settings.finish === "checker" ? VIEWER_TEXTURE_SOURCE : undefined
            }
          >
            {({ mesh: source, texture }) => (
              <Entity id={id}>
                <Transform
                  x={x}
                  y={y}
                  sx={scale}
                  sy={scale}
                  sz={scale}
                  rx={
                    mesh === "plane" || mesh === "planeOutline"
                      ? Math.PI / 4
                      : 0
                  }
                />
                <MeshInstance source={source} />
                <UnlitMaterial
                  {...(color ? { r: color[0], g: color[1], b: color[2] } : {})}
                />
                {texture !== undefined && <UnlitTexture source={texture} />}
              </Entity>
            )}
          </ReadyGeometry>
        );
      })}
    </World>
  );
}
