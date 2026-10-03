import {
  sameOutputReference,
  type OutputReference,
  type PickingWorldClient,
  type PresentationView,
  type RenderWorldClient,
} from "@ipp/client";
import type { IppCanvasHandle } from "@ipp/react/web";
import type { GuiUnhandledInputGate } from "@ipp/react/gui";
import { useEffect, useRef, useState } from "react";
import { type CameraView } from "./shared/camera.js";
import { installCameraControls } from "./shared/camera-controls.js";
import type {
  GalleryAssets,
  GalleryOptions,
  GallerySceneDefinition,
  GallerySceneMount,
} from "./shared/scene.js";

declare global {
  interface Window {
    ippWorldCanvas?: IppCanvasHandle;
  }
}

type GalleryClient = PickingWorldClient & RenderWorldClient;
type PickHandler = Parameters<typeof installCameraControls>[1]["picked"];

/**
 * The count of outstanding pick, projection and camera requests. It stays out
 * of the gallery's React state: a camera gesture sends a request every frame,
 * and only the readout that shows the count re-renders, never the scene.
 */
export class PendingRequests {
  private count = 0;
  private readonly listeners = new Set<() => void>();

  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  readonly current = (): number => this.count;

  change(delta: number): void {
    this.count += delta;
    for (const listener of this.listeners) listener();
  }
}

/** Browser navigation and presentation wrap the same mounts as native sessions. */
export function useGallery(
  scene: GallerySceneDefinition,
  options: GalleryOptions,
  assets: GalleryAssets,
  contract: Readonly<Record<string, unknown>>,
  onNavigate: (page: CameraView) => void,
  retainOptions: (page: CameraView, options: GalleryOptions) => void,
) {
  const [handle, setHandle] = useState<IppCanvasHandle>();
  const [mounted, setMounted] = useState<{
    handle: IppCanvasHandle;
    mount: GallerySceneMount;
  }>();
  const [view, setView] = useState<PresentationView | null>(null);
  const [sceneReady, setSceneReady] = useState(false);
  const [frameReady, setFrameReady] = useState(false);
  const [transitioning, setTransitioning] = useState(false);
  const [, changed] = useState(0);
  const canvas =
    mounted &&
    sceneReady &&
    frameReady &&
    view &&
    sameOutput(view.binding.output, mounted.mount.output)
      ? mounted.handle
      : undefined;
  const canvasFrame = useRef<HTMLDivElement>(null);
  const cameraControls = useRef<(() => void) | undefined>(undefined);
  const page = scene.id;
  const [pendingPicks] = useState(() => new PendingRequests());
  const [error, setError] = useState<string>();
  const currentOptions = useRef(options);
  currentOptions.current = options;
  const mountedRef = useRef<GallerySceneMount | undefined>(undefined);
  const mounting = useRef(Promise.resolve());
  const cleanupFailures = useRef(new WeakMap<IppCanvasHandle, unknown>());
  const disposeScene = useRef<(() => Promise<void>) | undefined>(undefined);

  useEffect(() => {
    if (!handle) return;
    const startup = new AbortController();
    let owned: GallerySceneMount | undefined;
    let unsubscribe: (() => void) | undefined;
    setSceneReady(false);
    setFrameReady(false);
    setMounted(undefined);
    const report = (failure: unknown) => {
      if (!startup.signal.aborted) setError(message(failure));
    };
    const pending = mounting.current.then(async () => {
      if (startup.signal.aborted) return;
      if (cleanupFailures.current.has(handle))
        throw cleanupFailures.current.get(handle);
      await assets.prepare(scene.resources ?? [], startup.signal);
      const mount = await scene.mount(
        {
          canvas: handle,
          assets,
          contract,
          signal: startup.signal,
          onError: report,
        },
        currentOptions.current,
      );
      owned = mount;
      if (startup.signal.aborted) return;
      mountedRef.current = mount;
      unsubscribe = mount.subscribe?.(() => changed((value) => value + 1));
      setMounted({ handle, mount });
      void mount.ready.then(() => {
        if (!startup.signal.aborted) setSceneReady(true);
      }, report);
    });
    mounting.current = pending.catch(report);
    let cleanup: Promise<void> | undefined;
    const dispose = () => {
      if (cleanup) return cleanup;
      startup.abort();
      unsubscribe?.();
      mountedRef.current = undefined;
      cleanup = mounting.current
        .then(async () => {
          await owned?.dispose();
        })
        .catch((failure: unknown) => {
          cleanupFailures.current.set(handle, failure);
          setError(message(failure));
          throw failure;
        });
      // The returned barrier keeps cleanup failure observable to navigation;
      // the mount queue retains its separate per-handle failure fence above.
      mounting.current = cleanup.then(
        () => {},
        () => {},
      );
      return cleanup;
    };
    disposeScene.current = dispose;
    return () => {
      if (disposeScene.current === dispose) disposeScene.current = undefined;
      void dispose().catch(() => {});
    };
  }, [handle, scene, assets, contract]);

  useEffect(() => {
    const mount = mountedRef.current;
    if (!mount) return;
    void mount
      .update(options)
      .catch((failure: unknown) => setError(message(failure)));
  }, [options]);

  useEffect(() => {
    if (
      !sceneReady ||
      !mounted ||
      !view ||
      !sameOutput(view.binding.output, mounted.mount.output)
    )
      return;
    let active = true;
    void Promise.resolve(mounted.mount.resize?.(view.binding.viewport))
      .then(() =>
        mounted.handle.frame({ afterOutputs: [mounted.mount.output] }),
      )
      .then(
        () => {
          if (active) {
            setFrameReady(true);
            setTransitioning(false);
          }
        },
        (failure: unknown) => {
          if (active) setError(message(failure));
        },
      );
    return () => {
      active = false;
    };
  }, [sceneReady, mounted, view]);

  useEffect(() => {
    if (!handle) return;
    // Inspection observes the connected primary World while resources prepare;
    // controls and frame capture still use the scene-ready canvas above.
    window.ippWorldCanvas = handle;
    return () => {
      delete window.ippWorldCanvas;
    };
  }, [handle]);

  async function navigate(nextPage: CameraView) {
    if (!canvas && !handle && !error) return;
    if (nextPage === page) {
      try {
        await mounted?.mount.action("resetCamera");
      } catch (failure) {
        setError(message(failure));
      }
      return;
    }
    cameraControls.current?.();
    setTransitioning(true);
    if (mounted && page !== "gui") retainOptions(page, mounted.mount.options);
    if (page === "platformer" || nextPage === "platformer") {
      try {
        // A new Canvas closes the previous Host. Remove declarations and
        // owned children while its old session can still acknowledge cleanup.
        await disposeScene.current?.();
      } catch (failure) {
        setTransitioning(false);
        setError(message(failure));
        return;
      }
      setHandle(undefined);
    }
    setMounted(undefined);
    setView(null);
    setSceneReady(false);
    setFrameReady(false);
    setError(undefined);
    onNavigate(nextPage);
    window.location.hash = nextPage;
  }

  return {
    canvas,
    handle,
    mount: mounted?.mount,
    output: mounted?.mount.output ?? null,
    viewChanged: setView,
    canvasFrame,
    cameraControls,
    page,
    switching: transitioning || (!!handle && !sceneReady),
    pendingPicks,
    error,
    setError,
    ready: setHandle,
    navigate,
  };
}

/** Shared camera gestures delegate object selection and dragging to the active world. */
export function useGalleryControls(
  gallery: ReturnType<typeof useGallery>,
  picked: PickHandler,
  enabled = true,
  unhandledInputGate?: GuiUnhandledInputGate,
) {
  const {
    canvas,
    canvasFrame,
    cameraControls,
    page,
    switching,
    pendingPicks,
    setError,
  } = gallery;
  useEffect(() => {
    const element = canvasFrame.current?.querySelector("canvas");
    if (!enabled || switching || !canvas || !element) return;
    const dispose = installCameraControls(element, {
      client: canvas.client as GalleryClient,
      picking: page === "lighting",
      pan: false,
      binding: () => canvas.view?.binding,
      flush: () => canvas.flush(),
      pending: (delta) => pendingPicks.change(delta),
      error: (failure) => setError(message(failure)),
      picked,
      ...(unhandledInputGate === undefined
        ? {}
        : guiAdmission(unhandledInputGate)),
    });
    cameraControls.current = dispose;
    return () => {
      dispose();
      if (cameraControls.current === dispose)
        cameraControls.current = undefined;
    };
  }, [
    canvas,
    canvasFrame,
    cameraControls,
    page,
    switching,
    picked,
    pendingPicks,
    setError,
    enabled,
    unhandledInputGate,
  ]);
}

/** Scene gestures proceed only when the runtime reports that GUI left them unhandled. */
function guiAdmission(gate: GuiUnhandledInputGate) {
  return {
    admitPointer(pointer: number, button: number, signal: AbortSignal) {
      const gui =
        button === 0
          ? "primary"
          : button === 1
            ? "auxiliary"
            : button === 2
              ? "secondary"
              : undefined;
      return gui === undefined
        ? Promise.resolve(false)
        : gate.pointerDown(pointer, gui, signal);
    },
    admitScroll(signal: AbortSignal) {
      return gate.scroll(signal);
    },
  };
}

function sameOutput(left: OutputReference, right: OutputReference): boolean {
  return sameOutputReference(left, right);
}

export function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
