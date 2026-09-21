import type {
  CameraMotion,
  GeometryPickResultEvent,
  PickingWorldClient,
  WorldPlane,
} from "@ipp/client";
import type { GuiPointerButton, GuiUnhandledInputGate } from "@ipp/react/gui";

export interface PickInteraction {
  click(): void;
  dragging(active: boolean): void;
  move(delta: [number, number, number]): void;
  finish?(): void;
}

interface Controls {
  client: PickingWorldClient;
  picking?: boolean;
  pan?: boolean;
  viewport(): { width: number; height: number };
  flush(): Promise<void>;
  picked(
    result: GeometryPickResultEvent,
    signal: AbortSignal,
  ): Promise<PickInteraction | undefined>;
  pending(delta: number): void;
  error(failure: unknown): void;
  /** Runtime-authoritative admission when this canvas also routes GUI input. */
  unhandledInputGate?: GuiUnhandledInputGate;
}

interface Gesture {
  pointer: number;
  button: number;
  startX: number;
  startY: number;
  x: number;
  y: number;
  sentX: number;
  sentY: number;
  width: number;
  height: number;
  dragged: boolean;
  dragging: boolean;
  released: boolean;
  finished: boolean;
  pick: "pending" | "hit" | "miss";
  abort: AbortController;
  interaction: PickInteraction | undefined;
  plane: WorldPlane | undefined;
  camera: bigint | undefined;
  viewport: { width: number; height: number };
  projecting: boolean;
  projectFrame: number;
  clicked: boolean;
  admission: "pending" | "admitted";
  ownsCapture: boolean;
}

/** Own browser gestures only. All camera pose/projection math stays in Rust. */
export function installCameraControls(
  canvas: HTMLCanvasElement,
  controls: Controls,
) {
  const gestures = new Set<Gesture>();
  let active: Gesture | undefined;
  let disposed = false;
  let frame = 0;
  let queued: CameraMotion | undefined;
  const wheelAdmissions = new Set<AbortController>();

  function setDragging(gesture: Gesture, dragging: boolean) {
    if (gesture.dragging === dragging) return;
    gesture.dragging = dragging;
    gesture.interaction?.dragging(dragging);
  }

  function finish(gesture: Gesture) {
    if (
      !gesture.released ||
      !gesture.finished ||
      gesture.admission === "pending" ||
      gesture.projecting ||
      gesture.projectFrame
    )
      return;
    if (!gesture.abort.signal.aborted && !gesture.dragged && !gesture.clicked) {
      gesture.clicked = true;
      gesture.interaction?.click();
    }
    if (gestures.delete(gesture)) gesture.interaction?.finish?.();
  }

  function moveObject(gesture: Gesture) {
    if (
      gesture.abort.signal.aborted ||
      !gesture.dragged ||
      !gesture.interaction ||
      !gesture.plane ||
      gesture.projecting ||
      gesture.projectFrame
    )
      return;
    if (!gesture.released) setDragging(gesture, true);
    if (gesture.x === gesture.sentX && gesture.y === gesture.sentY) return;
    gesture.projectFrame = requestAnimationFrame(() => {
      gesture.projectFrame = 0;
      void project(gesture);
    });
  }

  async function project(gesture: Gesture) {
    if (gesture.abort.signal.aborted || !gesture.plane || !gesture.interaction)
      return;
    const viewport = controls.viewport();
    const bounds = canvas.getBoundingClientRect();
    if (
      !bounds.width ||
      !bounds.height ||
      viewport.width !== gesture.viewport.width ||
      viewport.height !== gesture.viewport.height
    ) {
      cancel();
      return;
    }
    gesture.sentX = gesture.x;
    gesture.sentY = gesture.y;
    gesture.projecting = true;
    controls.pending(1);
    try {
      const result = await controls.client.query({
        type: "CameraProjectQuery",
        x: (gesture.sentX - bounds.left) / bounds.width,
        y: (gesture.sentY - bounds.top) / bounds.height,
        ...gesture.viewport,
        plane: gesture.plane,
      });
      if (gesture.abort.signal.aborted) return;
      if (!result.ok) throw new Error(result.error);
      if (result.camera !== gesture.camera) {
        cancel();
        return;
      }
      if (result.position) {
        const point = result.position;
        gesture.interaction.move([
          point[0] - gesture.plane.point[0],
          point[1] - gesture.plane.point[1],
          point[2] - gesture.plane.point[2],
        ]);
      }
    } catch (failure) {
      if (!gesture.abort.signal.aborted) {
        gesture.abort.abort();
        setDragging(gesture, false);
        controls.error(failure);
      }
    } finally {
      gesture.projecting = false;
      controls.pending(-1);
      moveObject(gesture);
      finish(gesture);
    }
  }

  function flushMotion() {
    cancelAnimationFrame(frame);
    frame = 0;
    const motion = queued;
    queued = undefined;
    if (!motion || disposed) return;
    try {
      controls.client.sendCommand({ type: "CameraNavigateCommand", motion });
    } catch (failure) {
      controls.error(failure);
    }
  }

  function queue(motion: CameraMotion) {
    // Combine raw samples before submission. A change of operation preserves order.
    if (queued?.kind === "rotate" && motion.kind === "rotate") {
      queued.yaw += motion.yaw;
      queued.pitch += motion.pitch;
    } else if (
      queued?.kind === "pan" &&
      motion.kind === "pan" &&
      queued.width === motion.width &&
      queued.height === motion.height
    ) {
      queued.x += motion.x;
      queued.y += motion.y;
    } else if (queued?.kind === "zoom" && motion.kind === "zoom") {
      queued.amount += motion.amount;
    } else {
      flushMotion();
      queued = motion;
    }
    if (!frame) frame = requestAnimationFrame(flushMotion);
  }

  function moveCamera(gesture: Gesture) {
    if (
      gesture.abort.signal.aborted ||
      gesture.admission !== "admitted" ||
      !gesture.dragged
    )
      return;
    if (gesture.button === 0 && gesture.pick !== "miss") return;
    const dx = gesture.x - gesture.sentX;
    const dy = gesture.y - gesture.sentY;
    if (!dx && !dy) return;
    gesture.sentX = gesture.x;
    gesture.sentY = gesture.y;
    queue(
      gesture.button === 1
        ? {
            kind: "pan",
            x: dx / gesture.width,
            y: dy / gesture.height,
            ...controls.viewport(),
          }
        : {
            kind: "rotate",
            yaw: (-dx / gesture.height) * Math.PI,
            pitch: (-dy / gesture.height) * Math.PI,
          },
    );
  }

  async function pick(gesture: Gesture, x: number, y: number) {
    controls.pending(1);
    try {
      await controls.flush();
      if (gesture.abort.signal.aborted) return;
      const result = await controls.client.query({
        type: "GeometryPickQuery",
        x,
        y,
        ...controls.viewport(),
        includeViewPlane: true,
      });
      if (gesture.abort.signal.aborted) return;
      if (!result.ok)
        throw new Error(
          result.error === "GeometryUnavailable"
            ? "Object geometry is still loading. Try again."
            : result.error,
        );
      gesture.pick = result.hit ? "hit" : "miss";
      gesture.plane = result.hit?.viewPlane;
      gesture.camera = result.camera ?? undefined;
      gesture.interaction = await controls.picked(result, gesture.abort.signal);
      if (gesture.abort.signal.aborted) {
        gesture.interaction?.finish?.();
        return;
      }
      // A short completed drag may precede the RPC result. Apply its buffered delta.
      moveCamera(gesture);
      moveObject(gesture);
    } catch (failure) {
      if (!gesture.abort.signal.aborted) controls.error(failure);
    } finally {
      gesture.finished = true;
      controls.pending(-1);
      finish(gesture);
    }
  }

  function release(gesture: Gesture) {
    gesture.released = true;
    setDragging(gesture, false);
    if (active === gesture) active = undefined;
    if (gesture.ownsCapture && canvas.hasPointerCapture(gesture.pointer))
      canvas.releasePointerCapture(gesture.pointer);
    finish(gesture);
  }

  function cancel(includeWheelAdmissions = true) {
    for (const gesture of gestures) {
      gesture.abort.abort();
      setDragging(gesture, false);
      cancelAnimationFrame(gesture.projectFrame);
      gesture.projectFrame = 0;
      gesture.interaction?.finish?.();
      gesture.interaction = undefined;
    }
    if (active) release(active);
    gestures.clear();
    if (includeWheelAdmissions) {
      for (const admission of wheelAdmissions) admission.abort();
      wheelAdmissions.clear();
    }
    cancelAnimationFrame(frame);
    frame = 0;
    queued = undefined;
  }

  function down(event: PointerEvent) {
    if (
      active ||
      !event.isPrimary ||
      (event.button !== 0 && (event.button !== 1 || controls.pan === false))
    )
      return;
    if (!controls.unhandledInputGate) event.preventDefault();
    flushMotion();
    // A new gesture supersedes any unresolved gesture from an earlier press.
    cancel();
    const bounds = canvas.getBoundingClientRect();
    if (!bounds.width || !bounds.height) return;
    const gesture: Gesture = {
      pointer: event.pointerId,
      button: event.button,
      startX: event.clientX,
      startY: event.clientY,
      x: event.clientX,
      y: event.clientY,
      sentX: event.clientX,
      sentY: event.clientY,
      width: bounds.width,
      height: bounds.height,
      dragged: false,
      dragging: false,
      released: false,
      finished: event.button === 1 || controls.picking === false,
      pick:
        event.button === 0 && controls.picking !== false ? "pending" : "miss",
      abort: new AbortController(),
      interaction: undefined,
      plane: undefined,
      camera: undefined,
      viewport: controls.viewport(),
      projecting: false,
      projectFrame: 0,
      clicked: false,
      admission: controls.unhandledInputGate ? "pending" : "admitted",
      ownsCapture: controls.unhandledInputGate === undefined,
    };
    active = gesture;
    gestures.add(gesture);
    if (gesture.ownsCapture) canvas.setPointerCapture(event.pointerId);
    if (controls.unhandledInputGate) {
      const button = guiButton(event.button);
      if (button === undefined) {
        gesture.abort.abort();
        release(gesture);
        gestures.delete(gesture);
        return;
      }
      void controls.unhandledInputGate
        .pointerDown(event.pointerId >>> 0, button, gesture.abort.signal)
        .then((admitted) => {
          if (gesture.abort.signal.aborted || !admitted) {
            gesture.abort.abort();
            if (active === gesture) active = undefined;
            gestures.delete(gesture);
            return;
          }
          gesture.admission = "admitted";
          if (gesture.button === 0 && controls.picking !== false) {
            void pick(
              gesture,
              (gesture.startX - bounds.left) / bounds.width,
              (gesture.startY - bounds.top) / bounds.height,
            );
          } else {
            gesture.finished = true;
            moveCamera(gesture);
            finish(gesture);
          }
        });
    } else if (event.button === 0 && controls.picking !== false)
      void pick(
        gesture,
        (event.clientX - bounds.left) / bounds.width,
        (event.clientY - bounds.top) / bounds.height,
      );
  }

  function sample(event: PointerEvent) {
    const gesture = active;
    if (!gesture || gesture.pointer !== event.pointerId) return;
    gesture.x = event.clientX;
    gesture.y = event.clientY;
    gesture.dragged ||=
      Math.hypot(gesture.x - gesture.startX, gesture.y - gesture.startY) >= 5;
    moveCamera(gesture);
    moveObject(gesture);
    // Mouse button chords can release the initiating button through pointermove.
    if ((event.buttons & (gesture.button === 1 ? 4 : 1)) === 0)
      release(gesture);
  }

  function up(event: PointerEvent) {
    if (
      !active ||
      active.pointer !== event.pointerId ||
      active.button !== event.button
    )
      return;
    const gesture = active;
    sample(event);
    if (!gesture.released) release(gesture);
  }

  function canceled(event: PointerEvent) {
    if (active?.pointer === event.pointerId) cancel();
  }

  function lostCapture(event: PointerEvent) {
    const gesture = active;
    if (!gesture || gesture.pointer !== event.pointerId) return;
    const buttonMask = gesture.button === 1 ? 4 : 1;
    if ((event.buttons & buttonMask) === 0) release(gesture);
    else cancel();
  }

  function wheel(event: WheelEvent) {
    event.preventDefault();
    const unit =
      event.deltaMode === WheelEvent.DOM_DELTA_LINE
        ? 16
        : event.deltaMode === WheelEvent.DOM_DELTA_PAGE
          ? canvas.clientHeight
          : 1;
    const motion: CameraMotion = {
      kind: "zoom",
      amount: Math.max(-1, Math.min(1, event.deltaY * unit * 0.0015)),
    };
    const apply = (): void => {
      if (disposed) return;
      // Wheel input ends a pending drag before zooming the same active camera.
      if (gestures.size) {
        flushMotion();
        cancel(false);
      }
      queue(motion);
    };
    if (!controls.unhandledInputGate) {
      apply();
      return;
    }
    const admission = new AbortController();
    wheelAdmissions.add(admission);
    void controls.unhandledInputGate
      .scroll(admission.signal)
      .then((admitted) => {
        wheelAdmissions.delete(admission);
        if (admitted && !admission.signal.aborted) apply();
      });
  }

  function auxiliary(event: MouseEvent) {
    if (event.button === 1) event.preventDefault();
  }

  canvas.addEventListener("pointerdown", down);
  canvas.addEventListener("pointermove", sample);
  canvas.addEventListener("pointerup", up);
  canvas.addEventListener("pointercancel", canceled);
  canvas.addEventListener("lostpointercapture", lostCapture);
  canvas.addEventListener("wheel", wheel, { passive: false });
  canvas.addEventListener("auxclick", auxiliary);
  const cancelAll = () => cancel();
  window.addEventListener("blur", cancelAll);
  window.addEventListener("resize", cancelAll);
  const stopCameraListener = controls.client.onCameraStateChanged(cancelAll);
  return () => {
    disposed = true;
    cancel();
    canvas.removeEventListener("pointerdown", down);
    canvas.removeEventListener("pointermove", sample);
    canvas.removeEventListener("pointerup", up);
    canvas.removeEventListener("pointercancel", canceled);
    canvas.removeEventListener("lostpointercapture", lostCapture);
    canvas.removeEventListener("wheel", wheel);
    canvas.removeEventListener("auxclick", auxiliary);
    window.removeEventListener("blur", cancelAll);
    window.removeEventListener("resize", cancelAll);
    stopCameraListener();
  };
}

function guiButton(button: number): GuiPointerButton | undefined {
  switch (button) {
    case 0:
      return "primary";
    case 1:
      return "auxiliary";
    case 2:
      return "secondary";
    default:
      return undefined;
  }
}
