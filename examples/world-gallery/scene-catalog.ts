import type { CameraView } from "./shared/camera.js";

export interface GalleryScene {
  readonly id: CameraView;
  readonly label: string;
  readonly shortLabel: string;
  readonly description: string;
}

/** Scene metadata shared by routing, the toolbar and the searchable picker. */
export const GALLERY_SCENES: readonly GalleryScene[] = [
  {
    id: "shapes",
    label: "Geometry",
    shortLabel: "Geometry",
    description:
      "Explore and edit IPP's built-in meshes, materials and transforms.",
  },
  {
    id: "lighting",
    label: "Lighting, Picking & Animation",
    shortLabel: "Lighting",
    description:
      "Select and move objects under animated lights and a bending beam.",
  },
  {
    id: "platformer",
    label: "Platformer Trail",
    shortLabel: "Platformer",
    description:
      "Follow a Blender-authored course with blended character motion.",
  },
  {
    id: "particles",
    label: "Particles",
    shortLabel: "Particles",
    description: "Tune a live fountain of light and tumbling mesh particles.",
  },
  {
    id: "gui",
    label: "GUI Demo",
    shortLabel: "GUI Demo",
    description:
      "Operate a projected React GUI with live runtime-owned controls.",
  },
];

export function galleryScene(id: CameraView): GalleryScene {
  return GALLERY_SCENES.find((scene) => scene.id === id)!;
}

export function gallerySceneFromHash(hash: string): CameraView {
  const id = hash.startsWith("#") ? hash.slice(1) : hash;
  return GALLERY_SCENES.some((scene) => scene.id === id)
    ? (id as CameraView)
    : "shapes";
}
