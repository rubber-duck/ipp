import type { GallerySceneDefinition } from "./shared/scene.js";
import { geometryScene } from "./worlds/geometry/scene.js";
import { lightingScene } from "./worlds/lighting/scene.js";
import { particlesScene } from "./worlds/particles/scene.js";
import { platformerScene } from "./worlds/platformer/scene.js";
import { guiScene } from "./worlds/gui/definition.js";

import { chartScene } from "./worlds/charts/scene.js";

/** Application scene modules consumed by both browser and native runners. */
export const GALLERY_SCENE_DEFINITIONS: readonly GallerySceneDefinition[] = [
  geometryScene,
  lightingScene,
  platformerScene,
  particlesScene,
  guiScene,
  chartScene,
];

export function gallerySceneDefinition(id: string): GallerySceneDefinition {
  const scene = GALLERY_SCENE_DEFINITIONS.find(
    (definition) => definition.id === id,
  );
  if (!scene) throw new Error(`Unknown gallery scene: ${id}`);
  return scene;
}
