import type {
  AnimationWorldClient,
  CameraWorldClient,
  Client,
} from "@ipp/client";
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
) {
  const client = base as AnimationWorldClient & CameraWorldClient;
  const response = await fetch(PLATFORMER_ASSETS + "manifest.json", { signal });
  if (!response.ok)
    throw new Error(`Platformer manifest: HTTP ${response.status}`);
  const manifest: BlenderDiskManifest = await response.json();
  if (manifest.format !== 1)
    throw new Error("Unsupported platformer asset manifest");
  signal.throwIfAborted();
  client.sendCommand({
    type: "RenderStateUpdateCommand",
    changes: { ambientLight: manifest.ambientLight },
  });
}
