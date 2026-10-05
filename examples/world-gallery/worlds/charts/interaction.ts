import { useEffect } from "react";
import type { IppCanvasHandle } from "@ipp/react/canvas";
import type { CameraViewMotion } from "@ipp/client";
import type { GallerySceneMount } from "../../shared/scene.js";

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
      navigationFrame = 0;
    let pending: { x: number; y: number } | null | undefined;
    let pendingMotion: CameraViewMotion | undefined;
    let pendingMotionGeneration = 0;
    let pressed:
      | {
          x: number;
          y: number;
          sentX: number;
          sentY: number;
          width: number;
          height: number;
          pointer: number;
          button: number;
        }
      | undefined;
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
    const flushHover = () => {
      frame = 0;
      if (disposed || pending === undefined) return;
      const point = pending;
      pending = undefined;
      void run("hover", point);
    };
    const queueHover = (point: typeof pending) => {
      pending = point;
      if (!frame) frame = requestAnimationFrame(flushHover);
    };
    const flushMotion = () => {
      cancelAnimationFrame(navigationFrame);
      navigationFrame = 0;
      const motion = pendingMotion;
      pendingMotion = undefined;
      if (
        motion &&
        !disposed &&
        pendingMotionGeneration === mount.options.cameraInputGeneration
      )
        void run("navigate", motion);
    };
    const queueMotion = (motion: CameraViewMotion) => {
      const generation = Number(mount.options.cameraInputGeneration);
      if (pendingMotion && pendingMotionGeneration !== generation)
        flushMotion();
      pendingMotionGeneration = generation;
      if (pendingMotion?.kind === "rotate" && motion.kind === "rotate") {
        pendingMotion.yaw += motion.yaw;
        pendingMotion.pitch += motion.pitch;
      } else if (pendingMotion?.kind === "pan" && motion.kind === "pan") {
        pendingMotion.x += motion.x;
        pendingMotion.y += motion.y;
      } else if (pendingMotion?.kind === "zoom" && motion.kind === "zoom") {
        pendingMotion.amount += motion.amount;
      } else {
        flushMotion();
        pendingMotion = motion;
      }
      if (!navigationFrame)
        navigationFrame = requestAnimationFrame(flushMotion);
    };
    const sample = (event: PointerEvent) => {
      if (!pressed || pressed.pointer !== event.pointerId) return;
      dragged ||=
        Math.hypot(event.clientX - pressed.x, event.clientY - pressed.y) >= 5;
      if (!dragged) return;
      const dx = event.clientX - pressed.sentX,
        dy = event.clientY - pressed.sentY;
      pressed.sentX = event.clientX;
      pressed.sentY = event.clientY;
      if (!dx && !dy) return;
      queueMotion(
        pressed.button === 1
          ? { kind: "pan", x: dx / pressed.width, y: dy / pressed.height }
          : {
              kind: "rotate",
              yaw: (-dx / pressed.height) * Math.PI,
              pitch: (-dy / pressed.height) * Math.PI,
            },
      );
    };
    const down = (event: PointerEvent) => {
      if (
        pressed ||
        (event.button !== 0 && event.button !== 1) ||
        !event.isPrimary
      )
        return;
      const bounds = element.getBoundingClientRect();
      if (!bounds.width || !bounds.height || !canvas.view?.binding) return;
      event.preventDefault();
      pressed = {
        x: event.clientX,
        y: event.clientY,
        sentX: event.clientX,
        sentY: event.clientY,
        width: bounds.width,
        height: bounds.height,
        pointer: event.pointerId,
        button: event.button,
      };
      dragged = false;
      element.setPointerCapture(event.pointerId);
      cancelAnimationFrame(frame);
      frame = 0;
      pending = undefined;
      void run("beginNavigation");
    };
    const move = (event: PointerEvent) => {
      if (pressed) {
        sample(event);
        if (!(event.buttons & (pressed.button === 1 ? 4 : 1))) up(event);
        return;
      }
      if (!event.buttons) queueHover(coordinates(event));
    };
    const up = (event: PointerEvent) => {
      if (!pressed || event.pointerId !== pressed.pointer) return;
      sample(event);
      const click =
        pressed.button === 0 &&
        !dragged &&
        Math.hypot(event.clientX - pressed.x, event.clientY - pressed.y) < 5;
      const pointer = pressed.pointer;
      pressed = undefined;
      if (element.hasPointerCapture(pointer))
        element.releasePointerCapture(pointer);
      // Include short gestures that start and finish before the browser's next frame.
      flushMotion();
      void run("endNavigation");
      if (click) void run("select", coordinates(event));
      queueHover(coordinates(event));
    };
    const cancel = () => {
      const navigating =
        pressed !== undefined ||
        (pendingMotion !== undefined &&
          pendingMotionGeneration === mount.options.cameraInputGeneration);
      const pointer = pressed?.pointer;
      pressed = undefined;
      if (pointer !== undefined && element.hasPointerCapture(pointer))
        element.releasePointerCapture(pointer);
      cancelAnimationFrame(navigationFrame);
      navigationFrame = 0;
      pendingMotion = undefined;
      cancelAnimationFrame(frame);
      frame = 0;
      pending = undefined;
      if (!disposed) {
        if (navigating) void run("beginNavigation");
        void run("hover", null);
      }
    };
    const leave = () => queueHover(null);
    const wheel = (event: WheelEvent) => {
      if (!canvas.view?.binding) return;
      event.preventDefault();
      if (pressed) cancel();
      const unit =
        event.deltaMode === WheelEvent.DOM_DELTA_LINE
          ? 16
          : event.deltaMode === WheelEvent.DOM_DELTA_PAGE
            ? element.clientHeight
            : 1;
      queueMotion({
        kind: "zoom",
        amount: Math.max(-1, Math.min(1, event.deltaY * unit * 0.0015)),
      });
    };
    const lostCapture = (event: PointerEvent) => {
      if (pressed?.pointer === event.pointerId && !event.buttons) up(event);
    };
    const auxiliary = (event: MouseEvent) => {
      if (event.button === 1) event.preventDefault();
    };
    element.addEventListener("pointerdown", down);
    element.addEventListener("pointermove", move);
    element.addEventListener("pointerup", up);
    element.addEventListener("pointercancel", cancel);
    element.addEventListener("pointerleave", leave);
    element.addEventListener("lostpointercapture", lostCapture);
    element.addEventListener("wheel", wheel, { passive: false });
    element.addEventListener("auxclick", auxiliary);
    window.addEventListener("blur", cancel);
    return () => {
      disposed = true;
      cancelAnimationFrame(frame);
      cancel();
      element.removeEventListener("pointerdown", down);
      element.removeEventListener("pointermove", move);
      element.removeEventListener("pointerup", up);
      element.removeEventListener("pointercancel", cancel);
      element.removeEventListener("pointerleave", leave);
      element.removeEventListener("lostpointercapture", lostCapture);
      element.removeEventListener("wheel", wheel);
      element.removeEventListener("auxclick", auxiliary);
      window.removeEventListener("blur", cancel);
    };
  }, [canvas, mount, enabled, report]);
}
