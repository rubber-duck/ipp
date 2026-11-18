import type { AssetReference } from "../assets/declarations.js";
import { useAsset } from "../scope/hooks.js";

/** Use the concrete loaded selection during the render that retires a declaration. */
export function useReadyBitmap(source?: AssetReference) {
  const asset = useAsset(source?.assetId ?? "");
  return asset?.status === "loaded" &&
    asset.current?.source === asset.pendingSource &&
    asset.current.kind === 2
    ? asset.current
    : undefined;
}
