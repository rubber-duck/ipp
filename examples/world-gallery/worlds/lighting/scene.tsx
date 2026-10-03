import type { AnimationPlaybackControl, CameraWorldClient } from "@ipp/client";
import { initializeCamera, setCameraView } from "../../shared/camera.js";
import { mountGalleryScene } from "../../shared/scene-mount.js";
import {
  GALLERY_SYSTEMS,
  type GallerySceneDefinition,
} from "../../shared/scene.js";
import {
  AnimationSession,
  type LightingPlaybackOptions,
} from "./animation-controller.js";
import {
  uploadRigAssets,
  releaseRigAssets,
  type AnimationModule,
} from "./animation-assets.js";
import {
  INITIAL_OBJECTS,
  type LightingWorldObjects,
  type ObjectId,
  type ObjectSettings,
} from "./model.js";
import type { WorldAnimationDemo } from "./playback.js";
import { LightingWorld } from "./world.js";

export const lightingScene: GallerySceneDefinition = {
  id: "lighting",
  label: "Lighting, Picking & Animation",
  shortLabel: "Lighting",
  description:
    "Select and move objects under animated lights and a bending beam.",
  defaultOptions: { objects: INITIAL_OBJECTS, selected: undefined },
  actions: [
    "select",
    "updateObject",
    "play",
    "pause",
    "stop",
    "restart",
    "seek",
    "configure",
    "resetCamera",
  ],
  resources: [
    { kind: 1, path: "/target/gallery-assets/spot-marker.mesh" },
    { kind: 1, path: "/target/gallery-assets/sun-marker.mesh" },
  ],
  world: () => ({ create: { selectedSystems: GALLERY_SYSTEMS } }),
  async mount(context, options) {
    await context.assets.prepare(lightingScene.resources!, context.signal);
    const client = context.canvas.client as CameraWorldClient;
    await uploadRigAssets(
      context.canvas.client as import("@ipp/client").AnimationWorldClient,
      context.contract as unknown as AnimationModule,
    );
    const camera = await initializeCamera(client);
    await setCameraView(client, camera, "lighting");
    const world = client.worldReference;
    if (!world) throw new Error("Lighting requires an explicit World");
    const output = await context.canvas.host.bindOutput(
      world,
      camera,
      "camera",
    );
    const abort = new AbortController();
    const signal = AbortSignal.any([context.signal, abort.signal]);
    let animation: WorldAnimationDemo = {
      session: undefined,
      players: [],
      error: undefined,
    };
    const listeners = new Set<() => void>();
    const notify = () => {
      for (const listener of listeners) listener();
    };
    let timer: ReturnType<typeof setTimeout> | undefined;
    let unsubscribe: (() => void) | undefined;
    let owned: AnimationSession | undefined;
    const control = async (
      action: AnimationPlaybackControl["action"],
      args?: unknown,
    ) => {
      await ready;
      const input = (args ?? {}) as {
        selection?: string;
        time?: number;
        speed?: number;
      };
      const selection = input.selection ?? "all";
      if (action === "seek")
        return owned!.control(selection, {
          action,
          time: Number(input.time ?? 0),
        });
      if (action === "playAtSpeed")
        return owned!.control(selection, {
          action,
          speed: Number(input.speed ?? 1),
        });
      return owned!.control(selection, { action });
    };
    const base = await mountGalleryScene(context, {
      output,
      options: {
        objects: structuredClone(INITIAL_OBJECTS),
        selected: undefined,
        ...options,
      },
      render: (state) => (
        <LightingWorld
          objects={state.objects as LightingWorldObjects}
          selected={state.selected as ObjectId | undefined}
        />
      ),
      actions: {
        async select(args) {
          await base.update({ selected: args });
        },
        async updateObject(args) {
          const { id, patch } = args as {
            id: ObjectId;
            patch: Partial<ObjectSettings>;
          };
          const objects = base.options.objects as LightingWorldObjects;
          if (!(id in objects))
            throw new Error(`Unknown lighting object: ${id}`);
          await base.update({
            objects: { ...objects, [id]: { ...objects[id], ...patch } },
          });
        },
        play: (args) => control("play", args),
        pause: (args) => control("pause", args),
        stop: (args) => control("stop", args),
        restart: (args) => control("restart", args),
        seek: (args) => control("seek", args),
        async configure(args) {
          await ready;
          const input = args as {
            selection?: string;
            field: "speed" | "looping";
            value: number | boolean;
          };
          await owned!.configure(
            input.selection ?? "all",
            input.field,
            input.value,
          );
        },
        async resetCamera() {
          await setCameraView(client, camera, "lighting");
        },
      },
    });
    const observe = async () => {
      if (signal.aborted || !owned) return;
      animation = {
        session: owned,
        players: await owned.observe(),
        error: undefined,
      };
      if (signal.aborted) return;
      notify();
      timer = setTimeout(() => {
        void observe().catch(report);
      }, 120);
    };
    const report = (failure: unknown) => {
      if (signal.aborted) return;
      const error =
        failure instanceof Error ? failure : new Error(String(failure));
      animation = { ...animation, error: error.message };
      notify();
      context.onError?.(error);
    };
    const ready = (async () => {
      owned = await AnimationSession.create(
        context.canvas,
        context.contract as unknown as AnimationModule,
        signal,
      );
      if (signal.aborted) {
        await owned.close();
        signal.throwIfAborted();
      }
      unsubscribe = (
        context.canvas.client as import("@ipp/client").AnimationWorldClient
      ).onPlaybackEvent((event) => {
        if (
          owned!.players.includes(event.controller.id) &&
          (event.kind === "failed" || event.kind === "invalidated")
        )
          report(new Error(event.reason ?? "Playback failed"));
      });
      await base.ready;
      if (options.animations)
        await owned.restorePlayback(
          options.animations as Readonly<
            Record<string, LightingPlaybackOptions>
          >,
        );
      await observe();
    })();
    void ready.catch(report);
    return {
      ...base,
      ready,
      get options() {
        return {
          ...base.options,
          ...(owned ? { animations: owned.playbackOptions } : {}),
        };
      },
      async update(patch) {
        await ready;
        await base.update(patch);
        if (patch.animations)
          await owned!.restorePlayback(
            patch.animations as Readonly<
              Record<string, LightingPlaybackOptions>
            >,
          );
      },
      get controller() {
        return animation;
      },
      subscribe(listener) {
        listeners.add(listener);
        return () => {
          listeners.delete(listener);
        };
      },
      async dispose() {
        abort.abort();
        clearTimeout(timer);
        unsubscribe?.();
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
        try {
          await releaseRigAssets(
            context.canvas.client as import("@ipp/client").AnimationWorldClient,
          );
        } catch (failure) {
          failures.push(failure);
        }
        listeners.clear();
        if (failures.length)
          throw new AggregateError(failures, "Gallery scene cleanup failed");
      },
    };
  },
};

export default lightingScene;
