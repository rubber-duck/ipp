/** Immutable Blender asset catalog shared by scene declarations and asset readiness checks. */
export const ASSET_ROOT = "/target/gallery-gui-assets/projector/";

export const PROJECTOR_POSE_SOURCES = {
  "cylinder-outside": `${ASSET_ROOT}frustum-cylinder-outside.ippm`,
  "cylinder-inside": `${ASSET_ROOT}frustum-cylinder-inside.ippm`,
  "sphere-outside": `${ASSET_ROOT}frustum-sphere-outside.ippm`,
  "sphere-inside": `${ASSET_ROOT}frustum-sphere-inside.ippm`,
} as const;

export const PROJECTOR_MESH_SOURCES = [
  `${ASSET_ROOT}shell.ippm`,
  `${ASSET_ROOT}trim.ippm`,
  `${ASSET_ROOT}aperture.ippm`,
  `${ASSET_ROOT}lens.ippm`,
  `${ASSET_ROOT}frustum.ippm`,
  `${ASSET_ROOT}base.ippm`,
  `${ASSET_ROOT}floor.ippm`,
  ...Object.values(PROJECTOR_POSE_SOURCES),
] as const;

export const PROJECTOR_TEXTURE_SOURCES = [
  `${ASSET_ROOT}metal-base.ippt`,
  `${ASSET_ROOT}base-baked.ippt`,
  `${ASSET_ROOT}floor-baked.ippt`,
] as const;
