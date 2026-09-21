import type { PickingWorldClient, RenderWorldClient } from "@ipp/client";
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

/** Authored worlds share a session; the disk scene owns a fresh loaded session. */
export function useGallery() {
  const [canvas, setCanvas] = useState<IppCanvasHandle>();
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
    if (
      !client.capabilities.gui ||
      !client.capabilities.surfaces ||
      !client.capabilities.picking ||
      !client.capabilities.skeletalAnimation ||
      !client.capabilities.animation
    ) {
      throw new Error(
        "The gallery requires GUI, picking, animation and skinning support",
      );
    }
    if (page === "platformer") {
      const state = await client.inspect();
      const camera = state.entities.find(
        (value) => value.metadata.symbolicId === "platformer-camera",
      );
      cameraId.current = camera?.id;
      cameraRest.current =
        camera?.base.find((value) => "qx" in value.fields)?.fields ?? {};
    } else {
      cameraId.current = await initializeCamera(client);
      if (page !== "shapes")
        await setCameraView(client, cameraId.current, page);
    }
    setSwitching(false);
    setCanvas(handle);
  }

  async function navigate(nextPage: CameraView) {
    if (!canvas || cameraId.current === undefined) return;
    cameraControls.current?.();
    const request = ++generation.current;
    setSwitching(true);
    const diskPage = (value: CameraView) => value === "platformer";
    if (page !== nextPage && (diskPage(page) || diskPage(nextPage))) {
      setCanvas(undefined);
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
        const transform = camera.base.find((value) => "qx" in value.fields)!;
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
      viewport: () => canvas.viewport,
      flush: () => canvas.flush(),
      pending: (delta) => setPendingPicks((count) => count + delta),
      error: (failure) => setError(message(failure)),
      picked,
      ...(unhandledInputGate === undefined ? {} : { unhandledInputGate }),
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

export function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
