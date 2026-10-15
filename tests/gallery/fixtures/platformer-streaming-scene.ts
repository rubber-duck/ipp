/** The real robot scene, with gait URLs routed to a gated fixture producer. */
import type { BlenderDiskManifest } from "../../../integrations/blender/client/disk-import.js";
import { geometryScene } from "../../../examples/demos/scenes/geometry/scene.js";
import { platformerScene } from "../../../examples/demos/scenes/platformer/scene.js";
import type {
  GallerySceneDefinition,
  GalleryAssets,
} from "../../../examples/demos/shared/scene.js";

export const streamingScene: GallerySceneDefinition = {
  ...platformerScene,
  async mount(context, options) {
    const prefix = String(options.streamPrefix);
    const assets: GalleryAssets = {
      prepare: context.assets.prepare.bind(context.assets),
      url: context.assets.url.bind(context.assets),
      readBytes: context.assets.readBytes.bind(context.assets),
      async readJson<T>(path: string, signal?: AbortSignal): Promise<T> {
        const value = await context.assets.readJson<T>(path, signal);
        if (!path.endsWith("manifest.json")) return value;
        const manifest = value as BlenderDiskManifest;
        return {
          ...manifest,
          clips: manifest.clips.map((clip) => ({
            ...clip,
            clip: {
              ...clip.clip,
              source: prefix + clip.clip.source.split("/").at(-1)!,
            },
          })),
        } as T;
      },
    };
    return platformerScene.mount({ ...context, assets }, options);
  },
};
export default streamingScene;

export { DemoApplication } from "../../../examples/demos/application.js";
export const GALLERY_SCENE_DEFINITIONS = [streamingScene, geometryScene];
export function gallerySceneDefinition(id: string) {
  const scene = GALLERY_SCENE_DEFINITIONS.find((entry) => entry.id === id);
  if (!scene) throw new Error(`Unknown streaming fixture scene ${id}`);
  return scene;
}
