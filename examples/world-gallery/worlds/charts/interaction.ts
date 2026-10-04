import { useEffect } from "react";
import type { IppCanvasHandle } from "@ipp/react/canvas";
import type { PickingWorldClient } from "@ipp/client";
import type { GallerySceneMount } from "../../shared/scene.js";
import { installCameraControls } from "../../shared/camera-controls.js";

/** Browser sampling delegates the same normalized actions native runners use. */
export function useChartInteraction(
  canvas: IppCanvasHandle | undefined,
  mount: GallerySceneMount | undefined,
  enabled: boolean,
  report: (failure: unknown) => void,
) {
  useEffect(() => {
    const element = document.querySelector<HTMLCanvasElement>(
      ".canvas-frame canvas",
    );
    if (!enabled || !canvas || !mount || !element) return;
    let disposed = false,
      frame = 0,
      hovering = false;
    let pending: { x: number; y: number } | null | undefined;
    let pressed: { x: number; y: number; pointer: number } | undefined;
    let dragged = false;
    const run = (name: string, args?: unknown) =>
      mount.action(name, args).catch((failure) => {
        if (!disposed) report(failure);
      });
    const coordinates = (event: PointerEvent) => {
      const bounds = element.getBoundingClientRect();
      return {
        x: (event.clientX - bounds.left) / bounds.width,
        y: (event.clientY - bounds.top) / bounds.height,
      };
    };
    const flushHover = async () => {
      frame = 0;
      if (disposed || hovering || pending === undefined) return;
      const point = pending;
      pending = undefined;
      hovering = true;
      try {
        await run("hover", point);
      } finally {
        hovering = false;
        if (!disposed && pending !== undefined)
          frame = requestAnimationFrame(() => {
            void flushHover();
          });
      }
    };
    const queueHover = (point: typeof pending) => {
      pending = point;
      if (!frame && !hovering)
        frame = requestAnimationFrame(() => {
          void flushHover();
        });
    };
    const down = (event: PointerEvent) => {
      if (event.button !== 0 || !event.isPrimary) return;
      pressed = {
        x: event.clientX,
        y: event.clientY,
        pointer: event.pointerId,
      };
      dragged = false;
      queueHover(null);
    };
    const move = (event: PointerEvent) => {
      if (pressed) {
        dragged ||=
          Math.hypot(event.clientX - pressed.x, event.clientY - pressed.y) >= 5;
        return;
      }
      if (!event.buttons) queueHover(coordinates(event));
    };
    const up = (event: PointerEvent) => {
      if (!pressed || event.pointerId !== pressed.pointer) return;
      const click =
        !dragged &&
        Math.hypot(event.clientX - pressed.x, event.clientY - pressed.y) < 5;
      pressed = undefined;
      if (click) void run("select", coordinates(event));
      queueHover(coordinates(event));
    };
    const cancel = () => {
      pressed = undefined;
      queueHover(null);
    };
    const leave = () => queueHover(null);
    const disposeControls = installCameraControls(element, {
      client: canvas.client as PickingWorldClient,
      picking: false,
      pan: true,
      binding: () => canvas.view?.binding,
      flush: () => canvas.flush(),
      async picked() {
        return undefined;
      },
      pending() {},
      error: report,
      async navigate(motion) {
        await run("navigate", motion);
      },
    });
    element.addEventListener("pointerdown", down);
    element.addEventListener("pointermove", move);
    element.addEventListener("pointerup", up);
    element.addEventListener("pointercancel", cancel);
    element.addEventListener("pointerleave", leave);
    window.addEventListener("blur", cancel);
    return () => {
      disposed = true;
      cancelAnimationFrame(frame);
      disposeControls();
      element.removeEventListener("pointerdown", down);
      element.removeEventListener("pointermove", move);
      element.removeEventListener("pointerup", up);
      element.removeEventListener("pointercancel", cancel);
      element.removeEventListener("pointerleave", leave);
      window.removeEventListener("blur", cancel);
    };
  }, [canvas, mount, enabled, report]);
}
