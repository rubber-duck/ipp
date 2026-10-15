/** Optional chart finishes use ordinary authored paints and materials. */
import {
  ShaderAsset,
  assetRef,
  type AssetReference,
} from "../assets/declarations.js";
import { CustomMaterial } from "../components.js";
import { Paint } from "../gui/components.js";
import { PaintShader, VertexShader, FragmentShader } from "../shaders.js";
import { glassColor } from "./palette.js";
import { glassMaterial } from "./material.js";
import type { GlassMaterial } from "./types.js";
import {
  GLASS_CHART_PAINT,
  GLASS_CHART_PAINT_PARAMETERS,
} from "./chart-paint.js";
import {
  GLASS_CHART_MESH_PARAMETERS,
  GLASS_CHART_MESH_RECIPE,
  GLASS_CHART_MESH_ATTRIBUTES,
  GLASS_CHART_MESH_VERTEX,
  GLASS_CHART_MESH_FRAGMENT,
} from "./chart-mesh.js";

type Vector = readonly [number, number, number, number];

export interface GlassChartFinishProps {
  /** Overrides the procedural sheen hue; omitted preserves each native data colour. */
  accent?: string;
  /** 2D: normalized local XY/depth/intensity. 3D: World XYZ/intensity. */
  light?: Vector;
  /** Chart-relative virtual direction, or an authored point in World coordinates. */
  lightSpace?: "chart" | "world";
  /** The same dielectric finish as GUI sheets. */
  material?: Partial<GlassMaterial>;
  tint?: string;
  sceneLighting?: boolean;
}

export interface GlassChartAssetsProps {
  /** Declare once per React root, then share the immutable stages among charts. */
  id?: string;
}

export function GlassChartAssets({
  id = "glass-chart",
}: GlassChartAssetsProps) {
  return (
    <>
      <ShaderAsset
        key={`${id}-paint`}
        id={`${id}-paint`}
        recipe={{ presentation: true, lighting: true }}
        parameters={GLASS_CHART_PAINT_PARAMETERS}
      >
        <PaintShader>{GLASS_CHART_PAINT}</PaintShader>
      </ShaderAsset>
      <ShaderAsset
        key={`${id}-mesh`}
        id={`${id}-mesh`}
        recipe={GLASS_CHART_MESH_RECIPE}
        parameters={GLASS_CHART_MESH_PARAMETERS}
      >
        <VertexShader requiredAttributes={GLASS_CHART_MESH_ATTRIBUTES}>
          {GLASS_CHART_MESH_VERTEX}
        </VertexShader>
        <FragmentShader requiredAttributes={GLASS_CHART_MESH_ATTRIBUTES}>
          {GLASS_CHART_MESH_FRAGMENT}
        </FragmentShader>
      </ShaderAsset>
    </>
  );
}

export function glassChartProperties(
  props: GlassChartFinishProps,
  spatial: boolean,
) {
  const light =
    props.light ?? (spatial ? [-4, 8, 6, 0.65] : [0.08, 0.08, 5, 0.65]);
  if (!light.every(Number.isFinite) || light[3]! < 0)
    throw new RangeError(
      "Glass chart light must be finite with nonnegative intensity",
    );
  const finish = glassMaterial(props.material);
  return {
    accent:
      props.accent === undefined ? [0, 0, 0, 0] : glassColor(props.accent),
    light,
    tint: glassColor(props.tint ?? "#769DA8"),
    finish: [finish.ior, finish.roughness, finish.thickness, finish.edge],
    lighting: [
      props.sceneLighting ? 1 : 0,
      props.lightSpace === "chart" ? 1 : 0,
      0,
      0,
    ],
  };
}

export interface GlassChartPaintProps extends GlassChartFinishProps {
  source?: AssetReference | string;
}

/** Put beside a 2D Plot declaration; only its native data marks consume this paint. */
export function GlassChartPaint({
  source = assetRef("glass-chart-paint"),
  ...finish
}: GlassChartPaintProps) {
  return <Paint source={source} {...glassChartProperties(finish, false)} />;
}

/** Put beside a 3D Plot declaration; axes, labels and picking remain native. */
export function GlassChartMaterial({
  source = assetRef("glass-chart-mesh"),
  ...finish
}: GlassChartPaintProps) {
  return (
    <CustomMaterial
      source={source}
      receives_light={true}
      alpha_mode={2}
      conservative_bounds={true}
      {...glassChartProperties(finish, true)}
    />
  );
}
