/** Real scene fixtures with explicit client-side lifecycle faults. */
import type { GallerySceneDefinition } from "../../../examples/demos/shared/scene.js";
import { geometryScene } from "../../../examples/demos/scenes/geometry/scene.js";
import { lightingScene } from "../../../examples/demos/scenes/lighting/scene.js";
export { DemoApplication } from "../../../examples/demos/application.js";

const mountFailure: GallerySceneDefinition = {
  ...geometryScene,
  id: "particles",
  shortLabel: "Failed mount",
  async mount() {
    throw new Error("application fixture mount failed");
  },
};
const sourceFailure: GallerySceneDefinition = {
  ...geometryScene,
  id: "gui",
  shortLabel: "Failed source",
  world() {
    throw new Error("application fixture source failed");
  },
};
const retryCleanup: GallerySceneDefinition = {
  ...geometryScene,
  id: "platformer",
  shortLabel: "Cleanup retry",
  async mount(context, options) {
    const mount = await geometryScene.mount(context, options);
    let attempts = 0;
    return {
      ...mount,
      async dispose() {
        if (++attempts === 1)
          throw new Error("application fixture cleanup failed");
        await mount.dispose();
        // A retired scene's delayed error must not overwrite its successor.
        context.onError?.(new Error("retired application fixture error"));
      },
    };
  },
};
export const GALLERY_SCENE_DEFINITIONS = [
  geometryScene,
  lightingScene,
  mountFailure,
  sourceFailure,
  retryCleanup,
];
export function gallerySceneDefinition(id: string) {
  const result = GALLERY_SCENE_DEFINITIONS.find((scene) => scene.id === id);
  if (!result) throw new Error(`Unknown application fixture ${id}`);
  return result;
}
