import {
  initialMeshSettings,
  type GeometryParameters,
  type IsolatedShape,
} from "../../shared/geometry-catalog.js";
import { linearToHex } from "../../shared/colors.js";

export type Vec3 = [number, number, number];
export const SCENE_OBJECTS = [
  { id: "lighting-cube", name: "Cube", shape: "cube" },
  { id: "lighting-sphere", name: "Sphere", shape: "sphere" },
  { id: "lighting-pillar", name: "Pill", shape: "pill" },
  { id: "lighting-spot", name: "Spotlight", shape: "cone" },
  { id: "lighting-point", name: "Point light", shape: "sphere" },
  { id: "lighting-fill", name: "Sun", shape: "plane" },
  { id: "lighting-skinning", name: "Skinned beam", shape: "cube" },
] as const;
export type ObjectId = (typeof SCENE_OBJECTS)[number]["id"];
export const ANIMATED_IDS = [
  "lighting-spot",
  "lighting-point",
  "lighting-fill",
  "lighting-skinning",
] as const;
export type AnimatedId = (typeof ANIMATED_IDS)[number];
export const isAnimated = (id: ObjectId): id is AnimatedId =>
  ANIMATED_IDS.some((value) => value === id);
export const isLight = (id: ObjectId) =>
  id === "lighting-spot" || id === "lighting-point" || id === "lighting-fill";

export interface ObjectSettings {
  position: Vec3;
  scale: number;
  color: string;
  roughness: number;
  metallic: number;
  castShadows: boolean;
  receiveShadows: boolean;
  parameters: GeometryParameters;
  intensity: number;
  range: number;
  innerCone: number;
  outerCone: number;
  markerSize: number;
}
export type LightingWorldObjects = Record<ObjectId, ObjectSettings>;

function settings(
  shape: IsolatedShape,
  position: Vec3,
  color: Vec3,
  parameters: Partial<GeometryParameters> = {},
): ObjectSettings {
  return {
    position,
    color: linearToHex(color),
    scale: 1,
    roughness: 0.38,
    metallic: 0.15,
    castShadows: true,
    receiveShadows: true,
    parameters: { ...initialMeshSettings(shape).parameters, ...parameters },
    intensity: 1,
    range: 18,
    innerCone: 0.42,
    outerCone: 0.75,
    markerSize: 1,
  };
}
export const INITIAL_OBJECTS: LightingWorldObjects = {
  "lighting-cube": settings("cube", [-1.35, 0.7, 0], [0.06, 0.42, 0.8], {
    width: 1.4,
    height: 1.4,
    length: 1.4,
  }),
  "lighting-sphere": settings("sphere", [0.5, 0.85, 0.35], [0.8, 0.23, 0.07], {
    radius: 0.85,
  }),
  "lighting-pillar": settings("pill", [1.7, 1.15, -1.15], [0.25, 0.7, 0.38], {
    radius: 0.4,
    height: 2.3,
  }),
  "lighting-spot": {
    ...settings("cone", [-3, 5, 3], [1, 0.88, 0.72]),
    intensity: 90,
  },
  "lighting-point": {
    ...settings("sphere", [2, 2, 2], [0.45, 0.6, 1]),
    intensity: 18,
    range: 10,
  },
  "lighting-fill": {
    ...settings("plane", [3, 4, -3], [0.5, 0.65, 1]),
    intensity: 0.65,
  },
  "lighting-skinning": settings("cube", [-1.5, 1, -2], [0.85, 0.6, 0.3], {
    width: 0.5,
    height: 2,
    length: 0.5,
  }),
};
