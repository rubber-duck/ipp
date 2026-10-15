import type { GallerySceneDefinition } from "./shared/scene.js";
import { geometryScene } from "./scenes/geometry/scene.js";
import { lightingScene } from "./scenes/lighting/scene.js";
import { particlesScene } from "./scenes/particles/scene.js";
import { platformerScene } from "./scenes/platformer/scene.js";

import { cockpitScene } from "./scenes/cockpit/scene.js";
import { chartScene } from "./scenes/charts/scene.js";

/** Application scene modules consumed by both browser and native runners. */
export const GALLERY_SCENE_DEFINITIONS: readonly GallerySceneDefinition[] = [
  geometryScene,
  lightingScene,
  platformerScene,
  particlesScene,
  chartScene,
  cockpitScene,
];

export function gallerySceneDefinition(id: string): GallerySceneDefinition {
  const scene = GALLERY_SCENE_DEFINITIONS.find(
    (definition) => definition.id === id,
  );
  if (!scene) throw new Error(`Unknown gallery scene: ${id}`);
  return scene;
}

export { DemoApplication } from "./application.js";
