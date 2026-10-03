import type { CameraWorldClient } from "@ipp/client";
import { ShapesWorld } from "./world.js";
import { INITIAL_CONTROLS, type ControlsState } from "./options.js";
import { initializeCamera, setCameraView } from "../../shared/camera.js";
import { mountGalleryScene } from "../../shared/scene-mount.js";
import {
  GALLERY_SYSTEMS,
  type GallerySceneDefinition,
} from "../../shared/scene.js";

export const geometryScene: GallerySceneDefinition = {
  id: "shapes",
  label: "Geometry",
  shortLabel: "Geometry",
  description:
    "Explore and edit IPP's built-in meshes, materials and transforms.",
  defaultOptions: { ...INITIAL_CONTROLS },
  actions: ["resetCamera"],
  world: () => ({ create: { selectedSystems: GALLERY_SYSTEMS } }),
  async mount(context, options) {
    const client = context.canvas.client as CameraWorldClient;
    const camera = await initializeCamera(client);
    await setCameraView(client, camera, "shapes");
    const world = client.worldReference;
    if (!world) throw new Error("Geometry requires an explicit World");
    const output = await context.canvas.host.bindOutput(
      world,
      camera,
      "camera",
    );
    return mountGalleryScene(context, {
      options: { ...INITIAL_CONTROLS, ...options },
      output,
      render(options) {
        const state = options as unknown as ControlsState;
        return state.mounted ? (
          <ShapesWorld shape={state.shape} meshes={state.meshes} />
        ) : null;
      },
      actions: {
        async resetCamera() {
          await setCameraView(client, camera, "shapes");
        },
      },
    });
  },
};
