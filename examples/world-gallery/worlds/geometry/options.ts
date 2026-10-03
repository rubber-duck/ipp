import {
  MESH_IDS,
  initialMeshSettings,
  type ViewerShape,
  type MeshSettings,
  type IsolatedShape,
} from "../../shared/geometry-catalog.js";

export interface ControlsState {
  readonly mounted: boolean;
  readonly shape: ViewerShape;
  readonly meshes: Readonly<Record<IsolatedShape, MeshSettings>>;
}

export const INITIAL_CONTROLS: ControlsState = {
  mounted: true,
  shape: "gallery",
  meshes: Object.fromEntries(
    MESH_IDS.map((shape) => [shape, initialMeshSettings(shape)]),
  ) as Record<IsolatedShape, MeshSettings>,
};
