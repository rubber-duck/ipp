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
import {
  initializeCamera,
  setCameraView,
  type CameraView,
} from "./shared/camera.js";
import { installCameraControls } from "./shared/camera-controls.js";
import { gallerySceneFromHash } from "./scene-catalog.js";

declare global {
  interface Window {
    ippWorldCanvas?: IppCanvasHandle;
  }
}

type GalleryClient = PickingWorldClient & RenderWorldClient;
type PickHandler = Parameters<typeof installCameraControls>[1]["picked"];

/** Authored worlds share a session; the disk scene owns a fresh loaded session.
 * Each session presents its gallery camera as the explicit Canvas root output. */
export function useGallery() {
  const [attached, setAttached] = useState<{
    handle: IppCanvasHandle;
    output: OutputReference;
  }>();
  const [view, setView] = useState<PresentationView | null>(null);
  const canvas =
    attached && view && sameOutput(view.binding.output, attached.output)
      ? attached.handle
      : undefined;
  const canvasFrame = useRef<HTMLDivElement>(null);
  const cameraControls = useRef<(() => void) | undefined>(undefined);
  const cameraId = useRef<bigint | undefined>(undefined);
  const cameraRest = useRef<Record<string, unknown>>({});
  const generation = useRef(0);
  const [page, setPage] = useState<CameraView>(() =>
    gallerySceneFromHash(window.location.hash),
  );
  const [switching, setSwitching] = useState(false);
  const [pendingPicks, setPendingPicks] = useState(0);
  const [error, setError] = useState<string>();

  useEffect(() => {
    if (!canvas) return;
    window.ippWorldCanvas = canvas;
    return () => {
      generation.current += 1;
      delete window.ippWorldCanvas;
    };
  }, [canvas]);

  async function ready(handle: IppCanvasHandle) {
    const client = handle.client as GalleryClient;
    const world = client.worldReference;
    if (!world) throw new Error("The gallery requires an explicit World");
    let camera: bigint;
    if (page === "platformer") {
      const state = await client.inspect();
      const saved = state.entities.find(
        (value) => value.metadata.symbolicId === "platformer-camera",
      );
      if (!saved) throw new Error("Saved platformer camera is missing");
      camera = saved.id;
      cameraRest.current =
        saved.components.find((value) => "qx" in value.fields)?.fields ?? {};
    } else {
      camera = await initializeCamera(client);
      if (page !== "shapes") await setCameraView(client, camera, page);
    }
    cameraId.current = camera;
    const output = await handle.host.bindOutput(world, camera, "camera");
    setSwitching(false);
    setAttached({ handle, output });
  }

  async function navigate(nextPage: CameraView) {
    if (!canvas || cameraId.current === undefined) return;
    cameraControls.current?.();
    const request = ++generation.current;
    setSwitching(true);
    const diskPage = (value: CameraView) => value === "platformer";
    if (page !== nextPage && (diskPage(page) || diskPage(nextPage))) {
      setAttached(undefined);
      setView(null);
      cameraId.current = undefined;
      setPage(nextPage);
      window.location.hash = nextPage;
      setError(undefined);
      return;
    }
    try {
      if (page === "platformer") {
        // Restore the imported camera placement after interactive orbit/zoom.
        const state = await canvas.client.inspect();
        const camera = state.entities.find(
          (value) => value.id === cameraId.current,
        )!;
        const transform = camera.components.find(
          (value) => "qx" in value.fields,
        )!;
        const result = await canvas.client.batch(
          Object.entries(cameraRest.current).map(([name, value]) => ({
            kind: "setField" as const,
            entity: { kind: "handle" as const, id: camera.id },
            component: transform.component,
            field: {
              offset: canvas.client.components.Transform!.fields[name]!.offset,
              value: { kind: "f32" as const, value: Number(value) },
            },
          })),
        );
        if (!result.ok) throw new Error(result.error.reason);
        return;
      }
      await setCameraView(
        canvas.client as GalleryClient,
        cameraId.current,
        nextPage,
      );
      if (generation.current !== request) return;
      setPage(nextPage);
      window.location.hash = nextPage;
      setError(undefined);
    } catch (failure) {
      if (generation.current === request) setError(message(failure));
    } finally {
      if (generation.current === request) setSwitching(false);
    }
  }

  return {
    canvas,
    output: attached?.output ?? null,
    viewChanged: setView,
    canvasFrame,
    cameraControls,
    page,
    switching,
    pendingPicks,
    setPendingPicks,
    error,
    setError,
    ready,
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
    setPendingPicks,
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
      pending: (delta) => setPendingPicks((count) => count + delta),
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
    setPendingPicks,
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
