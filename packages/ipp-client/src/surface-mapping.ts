// Planar mapping helpers for FlatSurface; curved charts require their provider mapping.

/**
 * Map a 2D point in FlatSurface plane coordinates ([0, width] x [0, height], +X right, +Y down)
 * to centred entity-local 3D coordinates (+X right, +Y up, front +Z).
 */
export function surfaceContentToEntityLocal(
  contentPoint: readonly [number, number],
  surfaceSize: readonly [number, number],
): [number, number, number] {
  return [
    contentPoint[0] - surfaceSize[0] * 0.5,
    surfaceSize[1] * 0.5 - contentPoint[1],
    0.0,
  ];
}

/**
 * Map a 2D point in centred entity-local coordinates to 2D FlatSurface plane coordinates.
 */
export function entityLocalToSurfaceContent(
  entityPoint: readonly [number, number],
  surfaceSize: readonly [number, number],
): [number, number] {
  return [
    entityPoint[0] + surfaceSize[0] * 0.5,
    surfaceSize[1] * 0.5 - entityPoint[1],
  ];
}
