/** Pure authoring recipes; the returned canonical fields are the only stored configuration. */
export type SurfaceFacing = "outside" | "inside";

export interface CurvedSurfaceFields {
  readonly width: number;
  readonly height: number;
  readonly curvature: number;
  readonly layer_spacing: number;
}

export interface SurfaceRadiusOptions {
  readonly width: number;
  readonly height: number;
  readonly radius: number;
  readonly facing?: SurfaceFacing;
  readonly layer_spacing?: number;
}

function positive(value: number, name: string): number {
  if (!Number.isFinite(value) || value <= 0)
    throw new RangeError(`${name} must be positive and finite`);
  return value;
}

/** Positive curvature faces outside; negative curvature faces inside. Author zero directly for flat. */
export function surfaceCurvatureFromRadius(
  radius: number,
  facing: SurfaceFacing = "outside",
): number {
  if (facing !== "outside" && facing !== "inside")
    throw new RangeError("facing must be outside or inside");
  const curvature = 1 / positive(radius, "radius");
  if (!Number.isFinite(curvature))
    throw new RangeError("radius must have a finite reciprocal");
  return facing === "inside" ? -curvature : curvature;
}

/** Physical chart lengths for either provider; the runtime validates shape domains and shell offsets. */
export function curvedSurfaceFromRadius({
  width,
  height,
  radius,
  facing = "outside",
  layer_spacing = 0,
}: SurfaceRadiusOptions): CurvedSurfaceFields {
  if (!Number.isFinite(layer_spacing))
    throw new RangeError("layer_spacing must be finite");
  return {
    width: positive(width, "width"),
    height: positive(height, "height"),
    curvature: surfaceCurvatureFromRadius(radius, facing),
    layer_spacing,
  };
}

export interface CylinderSurfaceAngleOptions {
  readonly radius: number;
  /** Horizontal arc angle in radians. Height is a straight physical distance. */
  readonly angle: number;
  readonly height: number;
  readonly facing?: SurfaceFacing;
  readonly layer_spacing?: number;
}

/** Cylinder chart width is radius times arc angle; the runtime validates the principal chart extent. */
export function cylinderSurfaceFromAngles({
  radius,
  angle,
  ...options
}: CylinderSurfaceAngleOptions): CurvedSurfaceFields {
  return curvedSurfaceFromRadius({
    ...options,
    radius,
    width: positive(radius, "radius") * positive(angle, "angle"),
  });
}

export interface SphereSurfaceAngleOptions {
  readonly radius: number;
  /** Centred-equidistant chart extents in radians, not latitude/longitude spans. */
  readonly angle_x: number;
  readonly angle_y: number;
  readonly facing?: SurfaceFacing;
  readonly layer_spacing?: number;
}

/** Sphere chart lengths are radius times angular extents; the runtime validates the principal chart domain. */
export function sphereSurfaceFromAngles({
  radius,
  angle_x,
  angle_y,
  ...options
}: SphereSurfaceAngleOptions): CurvedSurfaceFields {
  return curvedSurfaceFromRadius({
    ...options,
    radius,
    width: positive(radius, "radius") * positive(angle_x, "angle_x"),
    height: radius * positive(angle_y, "angle_y"),
  });
}
