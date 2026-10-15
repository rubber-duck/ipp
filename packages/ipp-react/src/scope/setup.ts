import type { Client, CreatedWorld, WorldCreateOptions } from "@ipp/client";
import type { ReactCompositionHost } from "../composition/attached-world.js";
import { createRoot, type ReactWorldRoot } from "../root.js";
import type { ReactWorldRootOptions } from "../reconciler/commits.js";

/** Opt-in authoring presets; an explicit selection replaces the preset completely. */
export type WorldPresetOptions = Omit<WorldCreateOptions, "selectedSystems"> & {
  selectedSystems?: readonly string[];
};

const canvasSystems = [
  "ipp.animation",
  "ipp.asset-dependencies",
  "ipp.lifecycle-publisher",
  "ipp.canvas",
] as const;
const guiSystems = [...canvasSystems, "ipp.gui", "ipp.gui-layout"] as const;
const sceneSystems = [
  "ipp.animation",
  "ipp.asset-dependencies",
  "ipp.lifecycle-publisher",
  "ipp.hierarchy",
  "ipp.look-at",
  "ipp.final-propagation",
  "ipp.geometry",
  "ipp.camera",
  "ipp.render",
] as const;

export function canvasWorldOptions(
  options: WorldPresetOptions = {},
): WorldCreateOptions {
  return {
    ...options,
    selectedSystems: options.selectedSystems ?? canvasSystems,
  };
}

export function guiWorldOptions(
  options: WorldPresetOptions = {},
): WorldCreateOptions {
  return { ...options, selectedSystems: options.selectedSystems ?? guiSystems };
}

export function sceneWorldOptions(
  options: WorldPresetOptions = {},
): WorldCreateOptions {
  return {
    ...options,
    selectedSystems: options.selectedSystems ?? sceneSystems,
  };
}

export interface OwnedReactWorldRoot {
  readonly world: CreatedWorld;
  readonly client: Client;
  readonly root: ReactWorldRoot;
  /** Unmount declarations, close their session and destroy this explicitly owned World. Retryable. */
  dispose(): Promise<void>;
}

/** Create exactly the requested World; output selection remains explicit. */
export async function createWorldRoot(
  host: ReactCompositionHost,
  create: WorldCreateOptions,
  options: Omit<ReactWorldRootOptions, "host"> = {},
): Promise<OwnedReactWorldRoot> {
  const world = await host.createWorld(create);
  let opened: Client | undefined;
  let root: ReactWorldRoot;
  try {
    opened = await host.openWorld(world.reference);
    root = createRoot(opened, { ...options, host });
  } catch (error) {
    const errors: unknown[] = [error];
    if (opened)
      await opened.close().catch((reason: unknown) => {
        errors.push(reason);
      });
    await host.destroyWorld(world.reference).catch((reason: unknown) => {
      errors.push(reason);
    });
    if (errors.length > 1)
      throw new AggregateError(errors, "React World setup and cleanup failed");
    throw error;
  }
  const client = opened;
  let disposed = false;
  let attempt: Promise<void> | undefined;
  return {
    world,
    client,
    root,
    dispose() {
      if (disposed) return Promise.resolve();
      if (attempt) return attempt;
      attempt = (async () => {
        await root.unmount();
        await client.close();
        await host.destroyWorld(world.reference);
        disposed = true;
      })().finally(() => {
        attempt = undefined;
      });
      return attempt;
    },
  };
}
