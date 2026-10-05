import type { CameraWorldClient } from "@ipp/client";
import { useLayoutEffect } from "react";
import { initializeCamera, setCameraView } from "../../shared/camera.js";
import { mountGalleryScene } from "../../shared/scene-mount.js";
import {
  GALLERY_SYSTEMS,
  type GalleryOptions,
  type GallerySceneContext,
  type GallerySceneDefinition,
} from "../../shared/scene.js";
import { frameGuiCamera } from "./camera.js";
import {
  PROJECTOR_MESH_SOURCES,
  PROJECTOR_TEXTURE_SOURCES,
} from "./projector-assets.js";
import {
  GuiWorld,
  openingSettings,
  useGuiScene,
  type GuiScene,
} from "./scene.js";

const OPTION_KEYS = [
  "accent",
  "exploded",
  "reducedMotion",
  "monitorWindow",
  "presentationTab",
  "layerStep",
  "workbenchTab",
  "autoscan",
  "surfaceCache",
  "surfaceShape",
  "surfaceFacing",
  "gain",
  "callsign",
  "shieldArmed",
  "vectorOnly",
  "tuning",
] as const;
const defaults = openingSettings();
const DEFAULT_OPTIONS = Object.fromEntries(
  OPTION_KEYS.map((key) => [key, key === "vectorOnly" ? false : defaults[key]]),
);

/** Scene controller and actual runtime declarations are shared by both runners. */
function GuiController({
  context,
  initialOptions,
  active,
  observe,
}: {
  readonly context: GallerySceneContext;
  readonly initialOptions: GalleryOptions;
  readonly active: boolean;
  readonly observe: (scene: GuiScene) => void;
}) {
  const scene = useGuiScene(
    context.canvas,
    true,
    context.assets,
    initialOptions,
  );
  useLayoutEffect(() => {
    observe(scene);
  }, [scene, observe]);
  return <GuiWorld scene={scene} active={active} />;
}

export const guiScene: GallerySceneDefinition = {
  id: "gui",
  label: "VESPER Scanner",
  shortLabel: "Scanner",
  description: "Log in to a local scanner projected into the scene.",
  defaultOptions: DEFAULT_OPTIONS,
  actions: [
    "pulse",
    "clearLog",
    "toggleShield",
    "toggleExplode",
    "toggleVectorOnly",
    "stopScan",
    "focusCallsign",
    "setAccent",
    "setExploded",
    "setReducedMotion",
    "setMonitorWindow",
    "setPresentationTab",
    "setLayerStep",
    "setShieldArmed",
    "setVectorOnly",
    "setWorkbenchTab",
    "selectSurfaceCache",
    "selectSurfaceShape",
    "selectSurfaceFacing",
    "setAutoscan",
    "setGain",
    "setCallsign",
    "tuning",
    "station",
    "app",
    "resetCamera",
  ],
  resources: [
    ...PROJECTOR_MESH_SOURCES.map((path) => ({ kind: 1, path })),
    ...PROJECTOR_TEXTURE_SOURCES.map((path) => ({ kind: 2, path })),
    { kind: 17, path: "/target/font-assets/shure-tech-mono.ippf" },
    { kind: 18, path: "/target/gallery-gui-assets/waveform.ippd" },
    { kind: 18, path: "/target/gallery-gui-assets/waveform-pulse.ippd" },
  ],
  world: () => ({ create: { selectedSystems: GALLERY_SYSTEMS } }),
  async mount(context, options) {
    await context.assets.prepare(guiScene.resources!, context.signal);
    const client = context.canvas.client as CameraWorldClient;
    const camera = await initializeCamera(client);
    await setCameraView(client, camera, "gui");
    const world = client.worldReference;
    if (!world) throw new Error("GUI requires an explicit World");
    const output = await context.canvas.host.bindOutput(
      world,
      camera,
      "camera",
    );
    const initialOptions = { ...DEFAULT_OPTIONS, ...options };
    let active = true;
    let controller: GuiScene | undefined;
    const listeners = new Set<() => void>();
    let resolveReady!: () => void;
    let rejectReady!: (failure: unknown) => void;
    const ready = new Promise<void>((resolve, reject) => {
      resolveReady = resolve;
      rejectReady = reject;
    });
    void ready.catch(() => {});
    const notify = () => {
      for (const listener of listeners) listener();
    };
    let unsubscribeState: (() => void) | undefined;
    let observedState: GuiScene["state"] | undefined;
    let framed = false;
    let cameraWork = Promise.resolve();
    let framingRequest = 0;
    const frameCurrentMode = (force = false) => {
      if (!active || !controller?.ready) return cameraWork;
      if (!force && framed) return cameraWork;
      framed = true;
      // Coalesce queued camera resets; disposal drains an in-flight write before
      // the next page can reuse the protected session camera.
      const request = ++framingRequest;
      cameraWork = cameraWork
        .catch(() => {})
        .then(async () => {
          if (!active || context.signal.aborted || request !== framingRequest)
            return;
          // Scene state never takes the camera from its user. Reset is explicit.
          await frameGuiCamera(
            client,
            camera,
            controller!.state.current.exploded,
          );
        });
      void cameraWork.catch((failure) => {
        controller?.reportFailure(failure);
        rejectReady(failure);
      });
      return cameraWork;
    };
    const observe = (scene: GuiScene) => {
      controller = scene;
      if (observedState !== scene.state) {
        unsubscribeState?.();
        observedState = scene.state;
        unsubscribeState = scene.state.subscribe(() => {
          void frameCurrentMode();
          notify();
        });
      }
      notify();
      if (scene.error) rejectReady(new Error(scene.error));
      else if (scene.ready)
        void frameCurrentMode().then(resolveReady, rejectReady);
    };
    const actions = Object.fromEntries(
      guiScene.actions
        .filter(
          (name) => !["resetCamera", "tuning", "station", "app"].includes(name),
        )
        .map((name) => [
          name,
          async (args?: unknown) => {
            await ready;
            const scene = controller!;
            const controls = {
              setGain: "gain",
              setAutoscan: "autoscan",
              setCallsign: "callsign",
              setExploded: "exploded",
              setReducedMotion: "reducedMotion",
              setLayerStep: "layerStep",
            } as const;
            const control = controls[name as keyof typeof controls];
            if (control)
              await scene.writeControl(
                control,
                args as boolean | number | string,
              );
            else if (name === "toggleExplode")
              await scene.writeControl(
                "exploded",
                !scene.state.current.exploded,
              );
            else if (name === "stopScan")
              await scene.writeControl("autoscan", false);
            else {
              const method = (controller as unknown as Record<string, unknown>)[
                name
              ];
              if (typeof method !== "function")
                throw new Error(`Unknown GUI action: ${name}`);
              await method(args);
            }
            await context.canvas.flush();
            notify();
          },
        ]),
    );
    for (const group of ["tuning", "station", "app"] as const) {
      actions[group] = async (args?: unknown) => {
        await ready;
        const { method, args: parameters = [] } = args as {
          method: string;
          args?: unknown[];
        };
        const action = (
          controller![group] as unknown as Record<string, unknown>
        )[method];
        if (typeof action !== "function")
          throw new Error(`Unknown GUI ${group} action: ${method}`);
        await action(...parameters);
        notify();
      };
    }
    actions.resetCamera = async () => {
      await ready;
      await frameCurrentMode(true);
    };
    const base = await mountGalleryScene(context, {
      output,
      options: initialOptions,
      render: () => (
        <GuiController
          context={context}
          initialOptions={initialOptions}
          active={active}
          observe={observe}
        />
      ),
      actions,
    });
    const abort = () => rejectReady(context.signal.reason);
    context.signal.addEventListener("abort", abort, { once: true });
    return {
      ...base,
      ready,
      get options() {
        const state = controller?.state.current;
        return state
          ? Object.fromEntries(OPTION_KEYS.map((key) => [key, state[key]]))
          : base.options;
      },
      get controller() {
        return controller;
      },
      subscribe(listener) {
        listeners.add(listener);
        return () => {
          listeners.delete(listener);
        };
      },
      async update(patch) {
        await ready;
        const scene = controller!;
        const setters: Readonly<
          Record<string, (value: unknown) => void | Promise<void>>
        > = {
          accent: (value) =>
            scene.setAccent(value as Parameters<GuiScene["setAccent"]>[0]),
          exploded: (value) => scene.writeControl("exploded", Boolean(value)),
          reducedMotion: (value) =>
            scene.writeControl("reducedMotion", Boolean(value)),
          monitorWindow: (value) =>
            scene.setMonitorWindow(
              value as Parameters<GuiScene["setMonitorWindow"]>[0],
            ),
          presentationTab: (value) =>
            scene.setPresentationTab(
              value as Parameters<GuiScene["setPresentationTab"]>[0],
            ),
          layerStep: (value) => scene.writeControl("layerStep", Number(value)),
          workbenchTab: (value) =>
            scene.setWorkbenchTab(
              value as Parameters<GuiScene["setWorkbenchTab"]>[0],
            ),
          autoscan: (value) => scene.writeControl("autoscan", Boolean(value)),
          surfaceCache: (value) =>
            scene.selectSurfaceCache(
              value as Parameters<GuiScene["selectSurfaceCache"]>[0],
            ),
          surfaceShape: (value) =>
            scene.selectSurfaceShape(
              value as Parameters<GuiScene["selectSurfaceShape"]>[0],
            ),
          surfaceFacing: (value) =>
            scene.selectSurfaceFacing(
              value as Parameters<GuiScene["selectSurfaceFacing"]>[0],
            ),
          gain: (value) => scene.writeControl("gain", Number(value)),
          callsign: (value) => scene.writeControl("callsign", String(value)),
          shieldArmed: (value) => {
            if (Boolean(value) !== scene.state.current.shieldArmed)
              scene.toggleShield();
          },
          vectorOnly: (value) => {
            if (Boolean(value) !== scene.state.current.vectorOnly)
              scene.toggleVectorOnly();
          },
          tuning: (value) =>
            scene.state.update({
              tuning: value as typeof scene.state.current.tuning,
            }),
        };
        for (const key of Object.keys(patch))
          if (!setters[key]) throw new Error(`Unknown GUI option: ${key}`);
        for (const [key, value] of Object.entries(patch))
          await setters[key]!(value);
        await context.canvas.flush();
      },
      async dispose() {
        rejectReady(new Error("GUI scene disposed"));
        context.signal.removeEventListener("abort", abort);
        const failures: unknown[] = [];
        active = false;
        unsubscribeState?.();
        try {
          await cameraWork;
        } catch (failure) {
          failures.push(failure);
        }
        try {
          // Keep the nested World mounted while removing its declarations;
          // unmount intentionally releases their ownership without deleting.
          await base.update({});
        } catch (failure) {
          if (failure !== context.signal.reason) failures.push(failure);
        }
        try {
          await base.dispose();
        } catch (failure) {
          failures.push(failure);
        }
        listeners.clear();
        if (failures.length)
          throw new AggregateError(failures, "GUI scene cleanup is incomplete");
      },
    };
  },
};

export default guiScene;
