import type {
  AnimationWorldClient,
  CameraWorldClient,
  Client,
} from "@ipp/client";
import type { GalleryAssets } from "../../shared/scene.js";
import type { BlenderDiskManifest } from "../../../../integrations/blender/client/disk-import.js";

export const PLATFORMER_ASSETS = "/target/gallery-platformer-assets/";
export const PLATFORMER_SOURCE = "https://platformer.ipp.invalid/";
export const PLATFORMER_WORLD = PLATFORMER_ASSETS + "platformer.ipp";
export const PLATFORMER_ROUTE = PLATFORMER_ASSETS + "route.json";

/** Restore the imported render state after the Host opens the saved World.
 * The gallery selects the saved camera as the Canvas root output. */
export async function initializePlatformerScene(
  base: Client,
  signal: AbortSignal,
  assets: GalleryAssets,
) {
  const client = base as AnimationWorldClient & CameraWorldClient;
  const manifest = await assets.readJson<BlenderDiskManifest>(
    PLATFORMER_ASSETS + "manifest.json",
    signal,
  );
  if (manifest.format !== 1)
    throw new Error("Unsupported platformer asset manifest");
  signal.throwIfAborted();
  client.sendCommand({
    type: "RenderStateUpdateCommand",
    changes: { ambientLight: manifest.ambientLight },
  });
}
