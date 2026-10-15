import { useContext, useId, useMemo } from "react";
import { AttachmentContext } from "../composition/attached-world.js";
import { assetRef, type AssetReference } from "../assets/declarations.js";
import { worldNamespace } from "./world.js";

/** Explicit IDs remain reconnectable; generated IDs follow this React instance. */
export function useEntityId(id?: string): string {
  const reactId = useId();
  const container = useContext(AttachmentContext);
  if (id !== undefined) {
    if (!id) throw new Error("Entity id must not be empty");
    return id;
  }
  return `ipp-react/${container ? worldNamespace(container) : "react"}/${reactId}`;
}

export function useAssetId(id?: string): string {
  const resolved = useEntityId(id);
  return id === undefined ? `${resolved}/asset` : resolved;
}

export function useAssetRef(id?: string): AssetReference {
  const resolved = useAssetId(id);
  return useMemo(() => assetRef(resolved), [resolved]);
}
