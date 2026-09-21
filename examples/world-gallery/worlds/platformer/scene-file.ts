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

/** Restore the imported presentation after the Host attaches the saved World. */
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
  const state = await client.inspect();
  signal.throwIfAborted();
  const camera = state.entities.find(
    (entity) => entity.metadata.symbolicId === "platformer-camera",
  );
  if (!camera) throw new Error("Saved platformer camera is missing");
  client.sendCommand({ type: "CameraActivateCommand", entity: camera.id });
  client.sendCommand({
    type: "RenderStateUpdateCommand",
    changes: { ambientLight: manifest.ambientLight },
  });
}
