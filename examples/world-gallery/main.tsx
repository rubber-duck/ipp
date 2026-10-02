import { useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import { browserRuntime } from "@ipp/client";
import { IppCanvas, World } from "@ipp/react/web";
import { createGuiUnhandledInputGate } from "@ipp/react/gui";
import { ShapesWorld } from "./worlds/geometry/world.js";
import {
  ShapesControls,
  GeometryLegend,
  INITIAL_CONTROLS,
  type ControlsState,
} from "./worlds/geometry/controls.js";
import type { MeshSettings } from "./shared/geometry-catalog.js";
import { LightingWorld } from "./worlds/lighting/world.js";
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
  useWorldAnimation,
} from "./worlds/lighting/animation.js";
import { GALLERY_RUNTIME } from "./shared/runtime.js";
import { CanvasFrameRate } from "./frame-rate.js";
import {
  message,
  useGallery,
  useGalleryControls,
} from "./gallery-controller.js";

import { ParticlesWorld } from "./worlds/particles/world.js";
import {
  ParticleControls,
  INITIAL_PARTICLES,
} from "./worlds/particles/controls.js";
import { GuiWorld, useGuiBlockers } from "./worlds/gui/scene.js";
import { GuiControls, useGuiScene } from "./worlds/gui/controls.js";
import { GUI_WHEEL_STEP } from "./worlds/gui/dashboard.js";
import {
  PLATFORMER_ASSETS,
  PLATFORMER_SOURCE,
  PLATFORMER_WORLD,
  initializePlatformerScene,
} from "./worlds/platformer/scene-file.js";
import { PlatformerWorld } from "./worlds/platformer/world.js";
import {
  PlatformerControls,
  usePlatformerScene,
} from "./worlds/platformer/controls.js";
import { ResponsiveControls, ScenePicker } from "./gallery-shell.js";
import { galleryScene } from "./scene-catalog.js";

/** Systems of the authored gallery World: animated and skinned meshes, lights
 * and particles under a camera, and the Surface anchor presenting the GUI panel
 * World. The panel World itself selects Canvas and GUI. */
const GALLERY_SYSTEMS = [
  "ipp.world-attachment",
  "ipp.lifecycle-publisher",
  "ipp.animation",
  "ipp.asset-dependencies",
  "ipp.skeleton",
  "ipp.skinning",
  "ipp.hierarchy",
  "ipp.look-at",
  "ipp.final-propagation",
  "ipp.geometry",
  "ipp.camera",
  "ipp.particles",
  "ipp.surface",
  "ipp.render",
] as const;

/** The DOM shell composes the gallery worlds and retains their authored state. */
export function Gallery() {
  const guiInputGate = useMemo(createGuiUnhandledInputGate, []);
  const gallery = useGallery();
  const {
    canvas,
    output,
    viewChanged,
    canvasFrame,
    page,
    switching,
    pendingPicks,
    error,
    setError,
    ready,
    navigate,
  } = gallery;
  const [controls, setControls] = useState<ControlsState>(INITIAL_CONTROLS);
  const [particles, setParticles] = useState(INITIAL_PARTICLES);
  const [objects, setObjects] = useState(INITIAL_OBJECTS);
  const [selected, setSelected] = useState<ObjectId>();
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
  const platformer = usePlatformerScene(canvas, page === "platformer");
  const gui = useGuiScene(canvas, page === "gui");
  const guiBlockers = useGuiBlockers(gui);
  const animation = useWorldAnimation(canvas, page === "lighting");
  const picked = useLightingInteraction(
    canvas,
    { objects, select: setSelected, update: updateObject },
    animation.session,
    () => setError(undefined),
  );
  useGalleryControls(
    gallery,
    picked,
    page !== "platformer",
    page === "gui" ? guiInputGate : undefined,
  );
  const worldError =
    error ??
    (page === "lighting"
      ? animation.error
      : page === "platformer"
        ? platformer.error
        : page === "gui"
          ? gui.error
          : undefined);

  const status = worldError
    ? "error"
    : switching
      ? "switching"
      : canvas &&
          (page !== "lighting" || animation.session) &&
          (page !== "platformer" || platformer.session) &&
          (page !== "gui" || gui.ready)
        ? "ready"
        : "starting";
  const currentScene = galleryScene(page);

  return (
    <main className="viewer-shell" data-page={page}>
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
          disabled={!canvas || switching}
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
            world={
              page === "platformer"
                ? { load: { url: PLATFORMER_WORLD } }
                : { create: { selectedSystems: GALLERY_SYSTEMS } }
            }
            output={output}
            {...(page === "platformer"
              ? { initialize: initializePlatformerScene }
              : {})}
            id="stage"
            className="stage"
            aria-label="3D world"
            runtime={{
              ...browserRuntime(new URL(GALLERY_RUNTIME, window.location.href)),
              ...(page === "platformer"
                ? {
                    resourceUrls: [
                      {
                        prefix: PLATFORMER_SOURCE,
                        baseUrl: new URL(
                          PLATFORMER_ASSETS,
                          window.location.href,
                        ).href,
                      },
                    ],
                  }
                : {}),
            }}
            // IppCanvas fixes guiInput when the canvas starts (K12), so the
            // authored canvas opens it on every page: the GUI page then gets
            // its input gate even when the gallery started on another page.
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
            onReady={(handle) => {
              void ready(handle).catch((failure: unknown) =>
                setError(message(failure)),
              );
            }}
            onViewChange={viewChanged}
            onError={(failure) => setError(failure.message)}
          >
            {page === "platformer" ? (
              <PlatformerWorld onCommit={platformer.onCommit} />
            ) : (
              <>
                {/* Unmounting a React root deletes nothing, so the authored
                    pages share one mounted World root: leaving a page removes
                    its declarations, which deletes what it created. */}
                <World>
                  {page === "shapes" ? (
                    controls.mounted && (
                      <ShapesWorld
                        shape={controls.shape}
                        meshes={controls.meshes}
                      />
                    )
                  ) : page === "particles" ? (
                    <ParticlesWorld settings={particles} />
                  ) : page === "lighting" ? (
                    <LightingWorld objects={objects} selected={selected} />
                  ) : null}
                </World>
                <GuiWorld scene={gui} active={page === "gui"} />
              </>
            )}
          </IppCanvas>
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
          <PlatformerControls demo={platformer} />
        ) : page === "gui" ? (
          <GuiControls scene={gui} />
        ) : (
          <>
            <LightingControls
              objects={objects}
              selected={selected}
              select={setSelected}
              update={updateObject}
            />
            <AnimationControls demo={animation} selectedObject={selected} />
          </>
        )}
      </ResponsiveControls>
    </main>
  );
}

const mount = document.getElementById("app");
if (!mount) throw new Error("Missing #app");
const root = createRoot(mount);
root.render(<Gallery />);
window.addEventListener("pagehide", () => root.unmount(), { once: true });
