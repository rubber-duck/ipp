import { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import { browserRuntime } from "@ipp/client";
import { IppCanvas } from "@ipp/react/web";
import { createGuiUnhandledInputGate } from "@ipp/react/gui";
import {
  ShapesControls,
  GeometryLegend,
  INITIAL_CONTROLS,
  type ControlsState,
} from "./worlds/geometry/controls.js";
import type { MeshSettings } from "./shared/geometry-catalog.js";
import {
  LightingControls,
  SelectionSummary,
} from "./worlds/lighting/controls.js";
import {
  INITIAL_OBJECTS,
  type ObjectId,
  type ObjectSettings,
} from "./worlds/lighting/model.js";
import { useLightingInteraction } from "./worlds/lighting/interaction.js";
import {
  AnimationControls,
  type useWorldAnimation,
} from "./worlds/lighting/animation.js";
import { GALLERY_RUNTIME } from "./shared/runtime.js";
import { CanvasFrameRate } from "./frame-rate.js";
import { useGallery, useGalleryControls } from "./gallery-controller.js";

import {
  ParticleControls,
  INITIAL_PARTICLES,
} from "./worlds/particles/controls.js";
import { useGuiBlockers, type GuiScene } from "./worlds/gui/scene.js";
import { GuiControls } from "./worlds/gui/controls.js";
import { GUI_WHEEL_STEP } from "./worlds/gui/dashboard.js";
import {
  PlatformerControls,
  type usePlatformerScene,
} from "./worlds/platformer/controls.js";
import { ChartControls } from "./worlds/charts/controls.js";
import { ResponsiveControls, ScenePicker } from "./gallery-shell.js";
import { galleryScene, gallerySceneFromHash } from "./scene-catalog.js";
import { gallerySceneDefinition } from "./scene-registry.js";
import { browserGalleryAssets } from "./browser-assets.js";
import type { GalleryOptions } from "./shared/scene.js";
import {
  PLATFORMER_ASSETS,
  PLATFORMER_SOURCE,
} from "./worlds/platformer/scene-file.js";
import type { CameraView } from "./shared/camera.js";
import type { GuiPickingBlocker } from "@ipp/client";

const contract: Readonly<Record<string, unknown>> = await import(
  `${GALLERY_RUNTIME}generated.js`
);

/** The DOM shell composes the gallery worlds and retains their authored state. */
export function Gallery() {
  const guiInputGate = useMemo(createGuiUnhandledInputGate, []);
  const [page, setPage] = useState<CameraView>(() =>
    gallerySceneFromHash(window.location.hash),
  );
  const scene = gallerySceneDefinition(page);
  const assets = useMemo(
    () => browserGalleryAssets(new URL(window.location.href)),
    [],
  );
  const [controls, setControls] = useState<ControlsState>(INITIAL_CONTROLS);
  const [particles, setParticles] = useState(INITIAL_PARTICLES);
  const [objects, setObjects] = useState(INITIAL_OBJECTS);
  const [selected, setSelected] = useState<ObjectId>();
  const [retainedOptions, setRetainedOptions] = useState<
    Partial<Record<CameraView, GalleryOptions>>
  >({});
  const updateControls = (change: Partial<ControlsState>) =>
    setControls((current) => ({ ...current, ...change }));
  const updateMesh = (change: Partial<MeshSettings>) =>
    setControls((current) =>
      current.shape === "gallery"
        ? current
        : {
            ...current,
            meshes: {
              ...current.meshes,
              [current.shape]: { ...current.meshes[current.shape], ...change },
            },
          },
    );
  const updateObject = (id: ObjectId, patch: Partial<ObjectSettings>) =>
    setObjects((current) => ({
      ...current,
      [id]: { ...current[id], ...patch },
    }));
  const options = useMemo(
    () =>
      page === "shapes"
        ? { ...controls }
        : page === "particles"
          ? { ...particles }
          : page === "lighting"
            ? { objects, selected }
            : (retainedOptions[page] ?? scene.defaultOptions),
    [page, controls, particles, objects, selected, scene, retainedOptions],
  );
  const gallery = useGallery(
    scene,
    options,
    assets,
    contract,
    setPage,
    (id, value) =>
      setRetainedOptions((current) => ({ ...current, [id]: value })),
  );
  const {
    canvas,
    output,
    viewChanged,
    canvasFrame,
    switching,
    pendingPicks,
    error,
    setError,
    ready,
    navigate,
  } = gallery;
  const platformer =
    page === "platformer"
      ? (gallery.mount?.controller as
          | ReturnType<typeof usePlatformerScene>
          | undefined)
      : undefined;
  const gui =
    page === "gui"
      ? (gallery.mount?.controller as GuiScene | undefined)
      : undefined;
  const animation = (
    page === "lighting" ? gallery.mount?.controller : undefined
  ) as ReturnType<typeof useWorldAnimation> | undefined;
  const [guiBlockers, setGuiBlockers] = useState<readonly GuiPickingBlocker[]>(
    [],
  );
  const picked = useLightingInteraction(
    canvas,
    { objects, select: setSelected, update: updateObject },
    animation?.session,
    () => setError(undefined),
  );
  useGalleryControls(
    gallery,
    picked,
    page !== "platformer" && page !== "charts2d" && page !== "charts3d",
    page === "gui" ? guiInputGate : undefined,
  );
  const worldError =
    error ?? animation?.error ?? platformer?.error ?? gui?.error;
  const status = worldError
    ? "error"
    : switching
      ? "switching"
      : canvas
        ? "ready"
        : "starting";
  const currentScene = galleryScene(page);

  return (
    <main className="viewer-shell" data-page={page}>
      {gui && <GuiInputBridge scene={gui} changed={setGuiBlockers} />}
      <header className="gallery-toolbar">
        <div className="gallery-brand">
          <span className="brand-mark" aria-hidden="true">
            IPP
          </span>
          <div>
            <p className="eyebrow">Interactive scenes</p>
            <h1 id="viewer-title">World gallery</h1>
          </div>
        </div>
        <ScenePicker
          page={page}
          disabled={!gallery.handle && !worldError}
          navigate={navigate}
        />
        <output id="status" data-state={status} aria-live="polite">
          <span className="status-light" aria-hidden="true" />
          {worldError ??
            (switching
              ? "Changing scene…"
              : status === "ready"
                ? "World ready"
                : "Starting world…")}
        </output>
      </header>
      <section
        className="showcase"
        aria-labelledby="viewer-title"
        aria-label={`${currentScene.label} scene`}
      >
        <div
          ref={canvasFrame}
          className="canvas-frame pickable"
          aria-busy={
            (page === "platformer" || page === "gui") &&
            status !== "ready" &&
            !worldError
          }
        >
          <IppCanvas
            key={page === "platformer" ? page : "authored"}
            world={scene.world(assets)}
            output={output}
            initialize={(client, signal, host) =>
              scene.initialize?.(client, signal, host, assets)
            }
            id="stage"
            className="stage"
            aria-label="3D world"
            runtime={{
              ...browserRuntime(new URL(GALLERY_RUNTIME, window.location.href)),
              resourceUrls: [
                {
                  prefix: PLATFORMER_SOURCE,
                  baseUrl: assets.url(PLATFORMER_ASSETS),
                },
              ],
            }}
            // The browser owns physical input routing to presented scene outputs.
            {...(page !== "platformer"
              ? {
                  guiInput: {
                    unhandledInputGate: guiInputGate,
                    wheelStep: GUI_WHEEL_STEP,
                    blockers: guiBlockers,
                  },
                }
              : {})}
            canvasProps={{ id: "ipp-world-canvas" }}
            onReady={ready}
            onViewChange={viewChanged}
            onError={(failure) => setError(failure.message)}
          />
          {page === "platformer" && status !== "ready" && (
            <div
              id={`${page}-loading`}
              className="scene-loading"
              role={worldError ? "alert" : "status"}
              aria-live="polite"
            >
              <p className="eyebrow">Platformer</p>
              <h2>{worldError ? "Unable to load scene" : "Loading scene…"}</h2>
              <p>
                {worldError ?? "Preparing the runner, route and resources."}
              </p>
            </div>
          )}
          {page === "gui" && status !== "ready" && (
            <div
              id="gui-loading"
              className="scene-loading"
              role={worldError ? "alert" : "status"}
              aria-live="polite"
            >
              <p className="eyebrow">GUI Demo</p>
              <h2>
                {worldError ? "Unable to load GUI demo" : "Loading GUI demo…"}
              </h2>
              <p>
                {worldError ??
                  "Preparing the font, waveform drawings and dashboard."}
              </p>
            </div>
          )}
          <div className="frame-badge" aria-hidden="true">
            {currentScene.label}
          </div>
          <CanvasFrameRate canvas={canvas} />
        </div>
        <div className="scene-summary">
          {page === "shapes" ? (
            <GeometryLegend meshes={controls.meshes} />
          ) : page === "lighting" ? (
            <SelectionSummary selected={selected} pending={pendingPicks} />
          ) : null}
        </div>
      </section>
      <ResponsiveControls page={page}>
        {page !== "platformer" && (
          <button
            id="reset-camera"
            className="secondary-button"
            type="button"
            disabled={!canvas || switching}
            onClick={() => {
              void navigate(page);
            }}
          >
            Reset camera
          </button>
        )}
        {page === "shapes" ? (
          <ShapesControls
            controls={controls}
            updateControls={updateControls}
            updateMesh={updateMesh}
          />
        ) : page === "particles" ? (
          <ParticleControls
            settings={particles}
            update={(patch) =>
              setParticles((current) => ({ ...current, ...patch }))
            }
          />
        ) : page === "platformer" ? (
          platformer && <PlatformerControls demo={platformer} />
        ) : page === "gui" ? (
          gui && <GuiControls scene={gui} />
        ) : page === "charts2d" || page === "charts3d" ? (
          <ChartControls
            mount={gallery.mount}
            spatial={page === "charts3d"}
            report={setError}
          />
        ) : (
          <>
            <LightingControls
              objects={objects}
              selected={selected}
              select={setSelected}
              update={updateObject}
            />
            {animation && (
              <AnimationControls demo={animation} selectedObject={selected} />
            )}
          </>
        )}
      </ResponsiveControls>
    </main>
  );
}

/** Keeps browser input blockers subscribed to runtime-owned GUI state. */
function GuiInputBridge({
  scene,
  changed,
}: {
  readonly scene: GuiScene;
  readonly changed: (blockers: readonly GuiPickingBlocker[]) => void;
}) {
  const blockers = useGuiBlockers(scene);
  useEffect(() => {
    changed(blockers);
    return () => changed([]);
  }, [blockers, changed]);
  return null;
}

const mount = document.getElementById("app");
if (!mount) throw new Error("Missing #app");
const root = createRoot(mount);
root.render(<Gallery />);
window.addEventListener("pagehide", () => root.unmount(), { once: true });
