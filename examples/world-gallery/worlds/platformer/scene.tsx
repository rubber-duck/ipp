import type { CameraWorldClient } from "@ipp/client";
import { mountGalleryScene } from "../../shared/scene-mount.js";
import type { GallerySceneDefinition } from "../../shared/scene.js";
import { initializePlatformerScene, PLATFORMER_WORLD } from "./scene-file.js";
import {
  PlatformerSession,
  type PlatformerMode,
  type PlatformerPlayback,
} from "./session.js";
import type { PlatformerDemo } from "./playback.js";
import { PlatformerWorld } from "./world.js";

const INITIAL_PLAYBACK: PlatformerPlayback = {
  mode: "walk",
  direction: 1,
  playing: true,
};

export const platformerScene: GallerySceneDefinition = {
  id: "platformer",
  label: "Platformer Trail",
  shortLabel: "Platformer",
  description:
    "Follow a Blender-authored course with blended character motion.",
  defaultOptions: { ...INITIAL_PLAYBACK },
  actions: ["setMode", "reverse", "playback", "reset", "resetCamera"],
  world: (assets) => ({ load: { url: assets.url(PLATFORMER_WORLD) } }),
  initialize: (client, signal, _host, assets) =>
    initializePlatformerScene(client, signal, assets),
  async mount(context, options) {
    const client = context.canvas.client as CameraWorldClient;
    const state = await client.inspect();
    const camera = state.entities.find(
      (entity) => entity.metadata.symbolicId === "platformer-camera",
    );
    if (!camera || !client.worldReference)
      throw new Error("Saved platformer camera is missing");
    const rest = camera.components.find(
      (component) => "qx" in component.fields,
    );
    const output = await context.canvas.host.bindOutput(
      client.worldReference,
      camera.id,
      "camera",
    );
    const listeners = new Set<() => void>();
    const abort = new AbortController();
    const signal = AbortSignal.any([context.signal, abort.signal]);
    let owned: PlatformerSession | undefined;
    let resolveCommit!: () => void;
    const committed = new Promise<void>((resolve) => {
      resolveCommit = resolve;
    });
    let demo: PlatformerDemo = {
      session: undefined,
      playback: { ...INITIAL_PLAYBACK },
      error: undefined,
      setError(value) {
        demo = {
          ...demo,
          error: typeof value === "function" ? value(demo.error) : value,
        };
        notify();
      },
      onCommit: () => resolveCommit(),
    };
    const notify = () => {
      for (const listener of listeners) listener();
    };
    const apply = async () => {
      const current = base.options as unknown as PlatformerPlayback;
      await owned!.setMode(current.mode);
      if (current.direction !== demo.playback.direction) await owned!.reverse();
      await owned!.playback(current.playing);
    };
    const base = await mountGalleryScene(context, {
      output,
      options: { ...INITIAL_PLAYBACK, ...options },
      render: () => <PlatformerWorld onCommit={demo.onCommit} />,
      actions: {
        async setMode(args) {
          await mount.update({ mode: args as PlatformerMode });
        },
        async reverse() {
          await mount.update({
            direction: demo.playback.direction === 1 ? -1 : 1,
          });
        },
        async playback(args) {
          await mount.update({ playing: Boolean(args) });
        },
        async reset() {
          await ready;
          await owned!.reset();
        },
        async resetCamera() {
          if (!rest) throw new Error("Saved camera transform is missing");
          const result = await client.batch(
            Object.entries(rest.fields).map(([name, value]) => ({
              kind: "setField" as const,
              entity: { kind: "handle" as const, id: camera.id },
              component: rest.component,
              field: {
                offset: client.components.Transform!.fields[name]!.offset,
                value: { kind: "f32" as const, value: Number(value) },
              },
            })),
          );
          if (!result.ok) throw new Error(result.error.reason);
        },
      },
    });
    const ready = (async () => {
      await committed;
      signal.throwIfAborted();
      owned = await PlatformerSession.create(
        context.canvas,
        context.assets,
        output,
        signal,
        (playback) => {
          demo = { ...demo, playback };
          notify();
        },
      );
      demo = { ...demo, session: owned };
      await apply();
      await base.ready;
      notify();
    })();
    void ready.catch((failure: unknown) => {
      if (signal.aborted) return;
      const error =
        failure instanceof Error ? failure : new Error(String(failure));
      demo.setError(error.message);
      context.onError?.(error);
    });
    const mount = {
      ...base,
      ready,
      get options() {
        return { ...base.options, ...demo.playback };
      },
      get controller() {
        return demo;
      },
      subscribe(listener: () => void) {
        listeners.add(listener);
        return () => {
          listeners.delete(listener);
        };
      },
      async update(patch: Readonly<Record<string, unknown>>) {
        await ready;
        await base.update(patch);
        await apply();
      },
      async dispose() {
        abort.abort();
        resolveCommit();
        await ready.catch(() => {});
        const failures: unknown[] = [];
        try {
          await owned?.close();
        } catch (failure) {
          failures.push(failure);
        }
        try {
          await base.dispose();
        } catch (failure) {
          failures.push(failure);
        }
        listeners.clear();
        if (failures.length)
          throw new AggregateError(failures, "Gallery scene cleanup failed");
      },
    };
    return mount;
  },
};

export default platformerScene;
