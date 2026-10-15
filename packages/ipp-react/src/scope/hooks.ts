import { useCallback, useSyncExternalStore } from "react";
import type { AssetReference } from "../assets/declarations.js";
import { useWorld } from "./world.js";

/** Registry snapshots report acknowledged resource state without suspending declarations. */
export function useAsset(reference: string | AssetReference) {
  const scope = useWorld();
  const id = typeof reference === "string" ? reference : reference.assetId;
  const snapshot = useCallback(() => scope.getAsset(id), [scope, id]);
  return useSyncExternalStore(scope.subscribeAssets, snapshot, snapshot);
}

export function useDataSource(name: string) {
  const scope = useWorld();
  const snapshot = useCallback(() => scope.getDataSource(name), [scope, name]);
  return useSyncExternalStore(scope.subscribeDataSources, snapshot, snapshot);
}

export function useCommitStatus() {
  const scope = useWorld();
  return useSyncExternalStore(
    scope.subscribeCommits,
    scope.getCommitStatus,
    scope.getCommitStatus,
  );
}
