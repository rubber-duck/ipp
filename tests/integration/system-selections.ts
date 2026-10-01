/** Explicit System selections for scenario Worlds.
 *
 * Every World creation names its Systems; there is no default selection. Each
 * part names one capability with its required predecessors, and scenarios
 * combine the parts they use with {@link selectSystems} rather than selecting
 * every registered System. */

/** Child World attachments. */
export const ATTACHMENTS = ["ipp.world-attachment"] as const;

/** Lifecycle watches and value observations. */
export const LIFECYCLE = ["ipp.lifecycle-publisher"] as const;

/** Property and structural animation. */
export const ANIMATION = ["ipp.animation"] as const;

/** Scalars and linear drivers. */
export const CONSTRAINTS = ["ipp.constraints"] as const;

/** Asset dependency tracking, which follows animation-held sources. */
export const ASSETS = ["ipp.animation", "ipp.asset-dependencies"] as const;

/** Transforms, terminal aiming and final World-space propagation. */
export const SPATIAL = [
  "ipp.hierarchy",
  "ipp.look-at",
  "ipp.final-propagation",
] as const;

/** Spatial propagation with bounds and picking geometry. */
export const GEOMETRY = [...ASSETS, ...SPATIAL, "ipp.geometry"] as const;

/** Camera outputs over evaluated geometry. */
export const CAMERA = [...GEOMETRY, "ipp.camera"] as const;

/** Meshes, materials and lights prepared for rendering. */
export const RENDER = [...GEOMETRY, "ipp.render"] as const;

/** Surface anchors, which require evaluated transforms and bounds. */
export const SURFACE = [...GEOMETRY, "ipp.surface"] as const;

/** Canvas output with styles, boxes and asset-backed text, drawings and bitmaps. */
export const CANVAS = [...ASSETS, "ipp.canvas"] as const;

/** GUI controls with entity layout, painted on a Canvas. */
export const GUI = [...CANVAS, "ipp.gui", "ipp.gui-layout"] as const;

/** Skeleton poses and skinned deformation. */
export const SKINNING = [
  ...ASSETS,
  ...SPATIAL,
  "ipp.skeleton",
  "ipp.skinning",
] as const;

/** Particle producers. */
export const PARTICLES = [...GEOMETRY, "ipp.particles"] as const;

/** A 3D scene: a camera over rendered, animated content. */
export const SCENE = [...CAMERA, ...RENDER] as const;

/** A React authoring root in the maintained fixtures: rendered and animated
 * scene content with Scalars, a camera, and Surface or spatial anchors for
 * attached child Worlds, observed through lifecycle watches. */
export const REACT_ROOT = [
  ...SCENE,
  ...SURFACE,
  ...ATTACHMENTS,
  ...CONSTRAINTS,
  ...LIFECYCLE,
] as const;

/** A React-created attached child World in the maintained fixtures: a camera
 * output with Scalar content and its own attached children, observed through
 * lifecycle watches. */
export const REACT_CHILD = [
  ...ATTACHMENTS,
  ...CAMERA,
  ...CONSTRAINTS,
  ...LIFECYCLE,
] as const;

/** Union of the named parts without duplicates, in first-named order. */
export function selectSystems(
  ...parts: readonly (readonly string[])[]
): string[] {
  return [...new Set(parts.flat())];
}
