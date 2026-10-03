import type { CameraWorldClient } from "@ipp/client";
import { initializeCamera, setCameraView } from "../../shared/camera.js";
import { mountGalleryScene } from "../../shared/scene-mount.js";
import {
  GALLERY_SYSTEMS,
  type GallerySceneDefinition,
} from "../../shared/scene.js";
import { INITIAL_PARTICLES, type ParticleSettings } from "./options.js";
import { ParticlesWorld } from "./world.js";

export const particlesScene: GallerySceneDefinition = {
  id: "particles",
  label: "Particles",
  shortLabel: "Particles",
  description: "Tune a live fountain of light and tumbling mesh particles.",
  defaultOptions: { ...INITIAL_PARTICLES },
  actions: ["restart", "toggleEmission", "resetCamera"],
  world: () => ({ create: { selectedSystems: GALLERY_SYSTEMS } }),
  async mount(context, options) {
    const client = context.canvas.client as CameraWorldClient;
    const camera = await initializeCamera(client);
    await setCameraView(client, camera, "particles");
    const world = client.worldReference;
    if (!world) throw new Error("Particles requires an explicit World");
    const output = await context.canvas.host.bindOutput(
      world,
      camera,
      "camera",
    );
    const mount = await mountGalleryScene(context, {
      output,
      options: { ...INITIAL_PARTICLES, ...options },
      render: (state) => (
        <ParticlesWorld settings={state as unknown as ParticleSettings} />
      ),
      actions: {
        async restart() {
          await mount.update({
            restart: Number(mount.options.restart) + 1,
            emitting: true,
          });
        },
        async toggleEmission() {
          await mount.update({ emitting: !mount.options.emitting });
        },
        async resetCamera() {
          await setCameraView(client, camera, "particles");
        },
      },
    });
    return mount;
  },
};

export default particlesScene;
