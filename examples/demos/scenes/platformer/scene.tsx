import type { CameraWorldClient, WorldReference } from "@ipp/client";
import type { CanvasWorldHandle, ReactWorldRoot } from "@ipp/react";
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
import { PlatformerControls, type PlatformerMovement } from "./controls.js";
import type { GuiKitContract } from "@ipp/react/gui-kit";

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
  controls: PlatformerControls,
  resources: [{ kind: 17, path: "/target/font-assets/shure-tech-mono.ippf" }],
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
    const target = state.entities
      .find(
        (entity) => entity.metadata.symbolicId === "platformer-camera-target",
      )
      ?.components.find((component) => "qx" in component.fields);
    const cameraComponent = client.components.Camera;
    if (!rest || !target || !cameraComponent)
      throw new Error("Saved following camera placement is missing");
    // Blender's DOF distance is independent of the authored viewing target.
    // Native orbit gestures should pivot around that target instead.
    const focusDistance = Math.hypot(
      ...["x", "y", "z"].map(
        (axis) => Number(rest.fields[axis]) - Number(target.fields[axis]),
      ),
    );
    const focused = await client.batch([
      {
        kind: "setField",
        entity: { kind: "handle", id: camera.id },
        component: cameraComponent.id,
        field: {
          offset: cameraComponent.fields.focus_distance!.offset,
          value: {
            kind: "f32",
            value: focusDistance,
          },
        },
      },
    ]);
    if (!focused.ok) throw new Error(focused.error.reason);
    const output = await context.canvas.host.bindOutput(
      client.worldReference,
      camera.id,
      "camera",
    );
    const listeners = new Set<() => void>();
    const abort = new AbortController();
    const signal = AbortSignal.any([context.signal, abort.signal]);
    let owned: PlatformerSession | undefined;
    let movementWorld: WorldReference | undefined;
    let declarationRoot: ReactWorldRoot | undefined;
    let resolveMovement!: () => void;
    const movementReady = new Promise<void>((resolve) => {
      resolveMovement = resolve;
    });
    const movementOpened = (handle: CanvasWorldHandle) => {
      movementWorld = handle.world;
      resolveMovement();
      notify();
    };
    let resolveCommit!: () => void;
    let rejectCommit!: (error: unknown) => void;
    const committed = new Promise<void>((resolve, reject) => {
      resolveCommit = resolve;
      rejectCommit = reject;
    });
    void committed.catch(() => {});
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
    const apply = async (patch?: Readonly<Record<string, unknown>>) => {
      const current = base.options as unknown as PlatformerPlayback;
      if (!patch || "mode" in patch) await owned!.setMode(current.mode);
      if (
        (!patch || "direction" in patch) &&
        current.direction !== demo.playback.direction
      )
        await owned!.reverse();
      if (!patch || "playing" in patch) await owned!.playback(current.playing);
    };
    const movement: PlatformerMovement = {
      read: () => demo,
      subscribe(listener) {
        listeners.add(listener);
        return () => {
          listeners.delete(listener);
        };
      },
      action: (name, args) => mount.action(name, args),
      report: (failure) => {
        rejectCommit(failure);
        demo.setError(
          failure instanceof Error ? failure.message : String(failure),
        );
      },
    };
    const base = await mountGalleryScene(context, {
      output,
      // PlatformerSession owns incoming clip readiness; pending replacements
      // must not block pause, newer gait requests or current-frame capture.
      awaitResourcesOnUpdate: false,
      options: { ...INITIAL_PLAYBACK, ...options },
      render: (current) => (
        <PlatformerWorld
          onCommit={(root) => {
            declarationRoot = root;
            demo.onCommit();
          }}
          movement={{
            camera: camera.id,
            contract: context.contract as unknown as GuiKitContract,
            font: context.assets.url(
              "/target/font-assets/shure-tech-mono.ippf",
            ),
            skin: current.skin === "cybertron" ? "cybertron" : "glass",
            movement,
            onReady: movementOpened,
          }}
        />
      ),
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
          await base.update({ direction: 1 });
        },
        async resetCamera() {
          if (!rest) throw new Error("Saved camera transform is missing");
          const result = await client.batch([
            ...Object.entries(rest.fields).map(([name, value]) => ({
              kind: "setField" as const,
              entity: { kind: "handle" as const, id: camera.id },
              component: rest.component,
              field: {
                offset: client.components.Transform!.fields[name]!.offset,
                value: { kind: "f32" as const, value: Number(value) },
              },
            })),
            {
              kind: "setField",
              entity: { kind: "handle", id: camera.id },
              component: cameraComponent.id,
              field: {
                offset: cameraComponent.fields.focus_distance!.offset,
                value: { kind: "f32", value: focusDistance },
              },
            },
          ]);
          if (!result.ok) throw new Error(result.error.reason);
        },
      },
    });
    const ready = (async () => {
      await committed;
      await movementReady;
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
        return { ...demo, movementWorld };
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
        await apply(patch);
      },
      async dispose() {
        abort.abort();
        resolveCommit();
        resolveMovement();
        await ready.catch(() => {});
        const failures: unknown[] = [];
        try {
          await owned?.close();
        } catch (failure) {
          failures.push(failure);
        }
        try {
          // Ordinary World scopes unmount without deleting declarations.
          // Remove the owned CanvasWorld while its authoring root is live.
          await declarationRoot?.render(null);
          await declarationRoot?.flush();
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
