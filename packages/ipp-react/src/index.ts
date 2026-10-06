export {
  AttachedWorld,
  AttachedWorldCleanupError,
} from "./composition/attached-world.js";
export type {
  AttachedWorldProps,
  AttachedWorldChild,
  AttachedWorldAttachment,
  AttachedWorldHandle,
  AttachedWorldCleanupJournal,
  AttachedWorldCleanupRecovery,
  ReactCompositionHost,
} from "./composition/attached-world.js";

export { CanvasWorld } from "./canvas/world.js";
export type {
  CanvasWorldHandle,
  CanvasWorldPresentation,
  CanvasWorldProps,
} from "./canvas/world.js";

export { ReactWorldBatchRejectedError } from "./reconciler/commits.js";
export { ReactWorldDuplicateEntityError } from "./reconciler/tree.js";
export type { ReactWorldRootOptions } from "./reconciler/commits.js";
export type { ReactWorldClient } from "./reconciler/world-client.js";
export type {
  EntityProps,
  ChildrenProps,
  EntityLinkProps,
  ParentJointProps,
  LookAtProps,
  ScalarProps,
  TransformProps,
  UnlitMaterialProps,
  PbrMaterialProps,
  CustomMaterialProps,
  LightProps,
  UnlitTextureProps,
  MeshInstanceProps,
  MeshPoseProps,
  CameraProps,
  PickingGeometryProps,
  BoundingGeometryProps,
  ComponentProps,
  FlatSurfaceProps,
  CylinderSurfaceProps,
  SphereSurfaceProps,
  SurfaceCacheProps,
} from "./components.js";
export {
  Entity,
  Children,
  EntityLink,
  ParentJoint,
  LookAt,
  Scalar,
  Transform,
  UnlitMaterial,
  PbrMaterial,
  CustomMaterial,
  Light,
  MeshInstance,
  MeshPose,
  UnlitTexture,
  Camera,
  PickingGeometry,
  BoundingGeometry,
  FlatSurface,
  CylinderSurface,
  SphereSurface,
  SurfaceCache,
} from "./components.js";

export { createRoot } from "./root.js";
export type { ReactWorldRoot } from "./root.js";

export {
  f32,
  i32,
  u32,
  bool,
  vec2,
  vec3,
  vec4,
  mat2,
  mat3,
  mat4,
  asset,
  texture2D,
} from "@ipp/client";
export type { DynamicPropertyInput } from "@ipp/client";

export { VertexShader, FragmentShader, PaintShader } from "./shaders.js";
export type { ShaderProps } from "./shaders.js";

export {
  Asset,
  AnimationAsset,
  ShaderAsset,
  assetRef,
  assetField,
} from "./assets/declarations.js";
export type {
  AssetProps,
  AnimationAssetProps,
  ShaderAssetProps,
  AssetReference,
  AssetFieldWrite,
} from "./assets/declarations.js";

export type { ReactAssetState } from "./assets/registry.js";

export { Animation } from "./animation/declarations.js";
export type {
  AnimationProps,
  AnimationHandle,
  AnimationBinding,
} from "./animation/declarations.js";

export {
  ParticleEmitter,
  ParticlePlayback,
  ParticleSprite,
  ParticleMesh,
} from "./components.js";
export type {
  ParticleEmitterProps,
  ParticlePlaybackProps,
  ParticleSpriteProps,
  ParticleMeshProps,
} from "./components.js";

export {
  DataSource,
  ColumnBindingAsset,
  BufferDataSourceBinding,
  StreamingDataSourceBinding,
  fixed,
  percent,
} from "./data/declarations.js";
export type {
  DataSourceProps,
  DataSourceHandle,
  ReactDataSourceState,
  ColumnBindingAssetProps,
  DataColumnBinding,
  DataColumnInterpolation,
  DataInterpolationKey,
  BufferDataSourceBindingProps,
  StreamingDataSourceBindingProps,
  DataWindow,
} from "./data/declarations.js";

export {
  PlotFrame2d,
  PlotFrame3d,
  PlotLine2d,
  PlotBars2d,
  PlotPie2d,
  PlotGridBars3d,
  PlotHeightSurface3d,
  PlotPoints3d,
  PlotPie3d,
} from "./plots/declarations.js";

export {
  PlotLegend,
  plotLegendSize,
  plotLegendPlacement,
  plotColorScaleColor,
} from "./plots/legend.js";
export type {
  PlotLegendColor,
  PlotLegendEntry,
  PlotColorScale,
  PlotLegendContent,
  PlotLegendStyle,
  PlotLegendProps,
  PlotLegendSide,
  PlotLegendOrigin,
  PlotLegendPlacementOptions,
  PlotLegendPlacement,
} from "./plots/legend.js";
export type {
  PlotContract,
  PlotSeries,
  PlotLabel,
  PlotRowsProps,
  PlotFrame2dProps,
  PlotFrame3dProps,
  PlotLine2dProps,
  PlotBars2dProps,
  PlotPie2dProps,
  PlotGridBars3dProps,
  PlotHeightSurface3dProps,
  PlotPoints3dProps,
  PlotPie3dProps,
} from "./plots/declarations.js";

export {
  curvedSurfaceFromRadius,
  cylinderSurfaceFromAngles,
  sphereSurfaceFromAngles,
  surfaceCurvatureFromRadius,
} from "@ipp/client";
export type {
  CurvedSurfaceFields,
  SurfaceFacing,
  SurfaceRadiusOptions,
  CylinderSurfaceAngleOptions,
  SphereSurfaceAngleOptions,
} from "@ipp/client";
