import {
  sameOutputReference,
  type CameraViewMotion,
  type RootBinding,
  type GeometryPickResultEvent,
  type PickingWorldClient,
  type WorldPlane,
} from "@ipp/client";

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
  binding(): RootBinding | undefined;
  flush(): Promise<void>;
  picked(
    result: GeometryPickResultEvent,
    signal: AbortSignal,
  ): Promise<PickInteraction | undefined>;
  pending(delta: number): void;
  error(failure: unknown): void;
  /** Application admission for scene input sharing a presented view. */
  admitPointer?(
    pointer: number,
    button: number,
    signal: AbortSignal,
  ): Promise<boolean>;
  admitScroll?(signal: AbortSignal): Promise<boolean>;
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
  binding: RootBinding;
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
  let queued: { binding: RootBinding; motion: CameraViewMotion } | undefined;
  const wheelAdmissions = new Set<AbortController>();

  function bindingIsCurrent(binding: RootBinding) {
    const current = controls.binding();
    return !disposed && current !== undefined && sameBinding(current, binding);
  }

  function releaseCapture(gesture: Gesture) {
    const ownsCapture = gesture.ownsCapture;
    gesture.ownsCapture = false;
    if (ownsCapture && canvas.hasPointerCapture(gesture.pointer))
      canvas.releasePointerCapture(gesture.pointer);
  }

  function cancelGesture(gesture: Gesture) {
    const interaction = gesture.interaction;
    const dragging = gesture.dragging;
    gesture.interaction = undefined;
    gesture.dragging = false;
    gesture.released = true;
    gestures.delete(gesture);
    if (active === gesture) active = undefined;
    cancelAnimationFrame(gesture.projectFrame);
    gesture.projectFrame = 0;
    releaseCapture(gesture);
    gesture.abort.abort();
    if (dragging) interaction?.dragging(false);
    interaction?.finish?.();
  }

  function gestureIsCurrent(gesture: Gesture) {
    if (!gesture.abort.signal.aborted && bindingIsCurrent(gesture.binding))
      return true;
    cancelGesture(gesture);
    return false;
  }

  function setDragging(gesture: Gesture, dragging: boolean) {
    if (dragging && !gestureIsCurrent(gesture)) return;
    if (gesture.dragging === dragging) return;
    gesture.dragging = dragging;
    gesture.interaction?.dragging(dragging);
  }

  function finish(gesture: Gesture) {
    if (!gestureIsCurrent(gesture)) return;
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
    if (gestures.delete(gesture)) {
      const interaction = gesture.interaction;
      gesture.interaction = undefined;
      interaction?.finish?.();
    }
  }

  function moveObject(gesture: Gesture) {
    if (
      !gestureIsCurrent(gesture) ||
      !gesture.dragged ||
      !gesture.interaction ||
      !gesture.plane ||
      gesture.projecting ||
      gesture.projectFrame
    )
      return;
    if (!gesture.released) {
      setDragging(gesture, true);
      if (!gestureIsCurrent(gesture)) return;
    }
    if (gesture.x === gesture.sentX && gesture.y === gesture.sentY) return;
    gesture.projectFrame = requestAnimationFrame(() => {
      gesture.projectFrame = 0;
      void project(gesture);
    });
  }

  async function project(gesture: Gesture) {
    if (!gestureIsCurrent(gesture) || !gesture.plane || !gesture.interaction)
      return;
    const bounds = canvas.getBoundingClientRect();
    if (!bounds.width || !bounds.height) {
      cancelGesture(gesture);
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
        view: { kind: "bound", binding: gesture.binding },
        plane: gesture.plane,
      });
      if (!gestureIsCurrent(gesture)) return;
      if (!result.ok) throw new Error(result.error);
      if (result.position) {
        const point = result.position;
        gesture.interaction.move([
          point[0] - gesture.plane.point[0],
          point[1] - gesture.plane.point[1],
          point[2] - gesture.plane.point[2],
        ]);
      }
    } catch (failure) {
      if (gestureIsCurrent(gesture)) {
        cancelGesture(gesture);
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
    const request = queued;
    queued = undefined;
    if (!request || !bindingIsCurrent(request.binding)) return;
    controls.pending(1);
    void controls.client
      .navigateCamera(request)
      .catch((failure: unknown) => {
        if (bindingIsCurrent(request.binding)) controls.error(failure);
      })
      .finally(() => controls.pending(-1));
  }

  function queue(motion: CameraViewMotion, binding: RootBinding) {
    if (queued && !sameBinding(queued.binding, binding)) flushMotion();
    const previous = queued?.motion;
    // Combine raw samples before submission. A change of operation preserves order.
    if (previous?.kind === "rotate" && motion.kind === "rotate") {
      previous.yaw += motion.yaw;
      previous.pitch += motion.pitch;
    } else if (previous?.kind === "pan" && motion.kind === "pan") {
      previous.x += motion.x;
      previous.y += motion.y;
    } else if (previous?.kind === "zoom" && motion.kind === "zoom") {
      previous.amount += motion.amount;
    } else {
      flushMotion();
      queued = { binding, motion };
    }
    if (!frame) frame = requestAnimationFrame(flushMotion);
  }

  function moveCamera(gesture: Gesture) {
    if (
      !gestureIsCurrent(gesture) ||
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
          }
        : {
            kind: "rotate",
            yaw: (-dx / gesture.height) * Math.PI,
            pitch: (-dy / gesture.height) * Math.PI,
          },
      gesture.binding,
    );
  }

  async function pick(gesture: Gesture, x: number, y: number) {
    controls.pending(1);
    try {
      await controls.flush();
      if (!gestureIsCurrent(gesture)) return;
      const result = await controls.client.query({
        type: "GeometryPickQuery",
        x,
        y,
        view: { kind: "bound", binding: gesture.binding },
        includeViewPlane: true,
      });
      if (!gestureIsCurrent(gesture)) return;
      if (!result.ok)
        throw new Error(
          result.error === "GeometryUnavailable"
            ? "Object geometry is still loading. Try again."
            : result.error,
        );
      gesture.pick = result.hit ? "hit" : "miss";
      gesture.plane = result.hit?.viewPlane;
      const interaction = await controls.picked(result, gesture.abort.signal);
      if (!gestureIsCurrent(gesture)) {
        interaction?.finish?.();
        return;
      }
      gesture.interaction = interaction;
      // A short completed drag may precede the RPC result. Apply its buffered delta.
      moveCamera(gesture);
      moveObject(gesture);
    } catch (failure) {
      if (gestureIsCurrent(gesture)) controls.error(failure);
    } finally {
      gesture.finished = true;
      controls.pending(-1);
      finish(gesture);
    }
  }

  function release(gesture: Gesture) {
    if (!gestureIsCurrent(gesture)) return;
    gesture.released = true;
    setDragging(gesture, false);
    if (active === gesture) active = undefined;
    releaseCapture(gesture);
    finish(gesture);
  }

  function cancel(includeWheelAdmissions = true) {
    for (const gesture of gestures) cancelGesture(gesture);
    if (includeWheelAdmissions) {
      for (const admission of wheelAdmissions) admission.abort();
      wheelAdmissions.clear();
    }
    cancelAnimationFrame(frame);
    frame = 0;
    queued = undefined;
  }

  function down(event: PointerEvent) {
    if (active) gestureIsCurrent(active);
    if (
      active ||
      !event.isPrimary ||
      (event.button !== 0 && (event.button !== 1 || controls.pan === false))
    )
      return;
    if (!controls.admitPointer) event.preventDefault();
    flushMotion();
    // A new gesture supersedes any unresolved gesture from an earlier press.
    cancel();
    const bounds = canvas.getBoundingClientRect();
    const binding = controls.binding();
    if (!bounds.width || !bounds.height || !binding) return;
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
      binding,
      projecting: false,
      projectFrame: 0,
      clicked: false,
      admission: controls.admitPointer ? "pending" : "admitted",
      ownsCapture: controls.admitPointer === undefined,
    };
    active = gesture;
    gestures.add(gesture);
    if (gesture.ownsCapture) canvas.setPointerCapture(event.pointerId);
    if (controls.admitPointer) {
      void controls
        .admitPointer(event.pointerId >>> 0, event.button, gesture.abort.signal)
        .then((admitted) => {
          if (!gestureIsCurrent(gesture) || !admitted) {
            cancelGesture(gesture);
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

  // The canvas's GUI input shares pointer capture on this element and releases
  // it once routing leaves the pointer to the scene, so losing capture while
  // the button is held only ends ownership; the browser reports a real
  // cancellation as pointercancel.
  function lostCapture(event: PointerEvent) {
    const gesture = active;
    if (!gesture || gesture.pointer !== event.pointerId) return;
    const buttonMask = gesture.button === 1 ? 4 : 1;
    if ((event.buttons & buttonMask) === 0) release(gesture);
    else gesture.ownsCapture = false;
  }

  function wheel(event: WheelEvent) {
    event.preventDefault();
    const unit =
      event.deltaMode === WheelEvent.DOM_DELTA_LINE
        ? 16
        : event.deltaMode === WheelEvent.DOM_DELTA_PAGE
          ? canvas.clientHeight
          : 1;
    const binding = controls.binding();
    if (!binding) return;
    const motion: CameraViewMotion = {
      kind: "zoom",
      amount: Math.max(-1, Math.min(1, event.deltaY * unit * 0.0015)),
    };
    const apply = (): void => {
      if (!bindingIsCurrent(binding)) return;
      // Wheel input ends a pending drag before zooming the same active camera.
      if (gestures.size) {
        flushMotion();
        cancel(false);
      }
      queue(motion, binding);
    };
    if (!controls.admitScroll) {
      apply();
      return;
    }
    const admission = new AbortController();
    wheelAdmissions.add(admission);
    void controls.admitScroll(admission.signal).then((admitted) => {
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
  };
}

function sameBinding(left: RootBinding, right: RootBinding): boolean {
  return (
    left.generation.host === right.generation.host &&
    left.generation.serial === right.generation.serial &&
    sameOutputReference(left.output, right.output) &&
    left.viewport.width === right.viewport.width &&
    left.viewport.height === right.viewport.height &&
    left.viewport.devicePixelRatio === right.viewport.devicePixelRatio
  );
}
