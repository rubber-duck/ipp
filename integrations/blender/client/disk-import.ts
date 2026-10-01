import type { WorldPersistenceHostClient } from "@ipp/client";
import {
  BLENDER_SYSTEMS,
  BlenderAdapter,
  type BlenderAssetIO,
  type BlenderClient,
  type BlenderContract,
  type ImportedClip,
} from "./adapter.js";
import type { BlenderSnapshot } from "./types.js";

export interface BlenderDiskManifest {
  format: 1;
  camera: string | null;
  ambientLight: [number, number, number];
  clips: { id: string; name: string; target: string; clip: ImportedClip }[];
}

/** Import any standard export through the receiving SDK, then use ordinary World serialization.
 * Resource publication happens before references enter the World; saveWorld never rewrites them.
 */
export async function importBlenderScene(
  host: WorldPersistenceHostClient<BlenderClient>,
  contract: BlenderContract,
  snapshot: BlenderSnapshot,
  assets: BlenderAssetIO,
  options: {
    symbolicId?: string;
    clipsOnly?: boolean;
  } = {},
) {
  const created = await host.createWorld({
    selectedSystems: BLENDER_SYSTEMS,
    ...(options.symbolicId ? { symbolicId: options.symbolicId } : {}),
  });
  let client: BlenderClient | undefined;
  let adapter: BlenderAdapter | undefined;
  let importFailure: unknown;
  try {
    client = await host.openWorld(created.reference);
    adapter = new BlenderAdapter(
      client,
      contract,
      new URL("https://localhost"),
      "",
      undefined,
      undefined,
      assets,
    );
    const applied = await adapter.apply(
      options.clipsOnly
        ? { ...snapshot, scene: { ...snapshot.scene, animations: [] } }
        : snapshot,
    );
    const entities = new Map(
      snapshot.scene.entities.map((entity) => [entity.id, entity]),
    );
    const clips = applied.clips.map((entry) => {
      const target = entities.get(entry.target)?.name;
      if (!target)
        throw new Error(
          `Reusable clip target ${entry.target} needs an exported name`,
        );
      return { id: entry.id, name: entry.name, target, clip: entry.clip };
    });
    const manifest: BlenderDiskManifest = {
      format: 1,
      camera: snapshot.scene.active_camera
        ? (entities.get(snapshot.scene.active_camera)?.name ?? null)
        : null,
      ambientLight: snapshot.scene.ambient_light ?? [0, 0, 0],
      clips,
    };
    const bytes = await host.saveWorld(client.session);
    return { bytes, manifest, entities: applied.entities.size };
  } catch (error) {
    importFailure = error;
    throw error;
  } finally {
    const cleanupFailures: unknown[] = [];
    for (const cleanup of [
      () => adapter?.dispose(),
      () => client?.close(),
      () => host.destroyWorld(created.reference),
    ]) {
      try {
        await cleanup();
      } catch (error) {
        cleanupFailures.push(error);
      }
    }
    if (cleanupFailures.length)
      throw new AggregateError(
        importFailure === undefined
          ? cleanupFailures
          : [importFailure, ...cleanupFailures],
        "Blender disk import cleanup failed",
      );
  }
}
