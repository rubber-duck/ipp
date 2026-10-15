import { GALLERY_SCENE_DEFINITIONS } from "./scene-registry.js";
import type { CameraView } from "./shared/camera.js";

export interface GalleryScene {
  readonly id: CameraView;
  readonly label: string;
  readonly shortLabel: string;
  readonly description: string;
}

/** Scene metadata stays with each shared definition. */
export const GALLERY_SCENES: readonly GalleryScene[] =
  GALLERY_SCENE_DEFINITIONS;

export function galleryScene(id: CameraView): GalleryScene {
  return GALLERY_SCENES.find((scene) => scene.id === id)!;
}

export function gallerySceneFromHash(hash: string): CameraView {
  const id = hash.startsWith("#") ? hash.slice(1) : hash;
  return GALLERY_SCENES.some((scene) => scene.id === id)
    ? (id as CameraView)
    : "shapes";
}
