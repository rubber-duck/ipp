import type { GuiInputCommand, GuiInputRoutingOutcome } from "@ipp/client";
import { createImeBridge, shouldSkipBeforeInput } from "./ime.js";
import {
  closeUnhandledInputGate,
  openUnhandledInputGate,
  settleUnhandledInputGateSubmission,
  trackUnhandledInputGate,
  type GuiUnhandledInputGate,
} from "./scene-input.js";

/** Browser pointer/keyboard/touch relay into GUI input contexts.
 *
 * Platform adapter for the optional `@ipp/react/gui` entry point: DOM
 * listeners translate mouse, touch, pen, wheel and keyboard events into
 * ordered [`BrowserGuiInputCommand`] payloads for an existing ordered
 * ingress sink. Headless use imports no DOM dependencies: the DOM is
 * touched only inside {@link attachCanvasGuiInput}.
 *
 * Ownership: the core `GuiInputSystem` owns focus, capture, hover and
 * gesture decisions; this module keeps no input state beyond the set of
 * live pointer identities needed to cancel on platform blur. It never
 * mutates scene state and never re-decides routing. Unhandled input stays
 * observable host-side for ordinary scene controls without duplicate
 * dispatch here. Browser composition (calling `attachCanvasGuiInput` on
 * the live canvas) stays in `@ipp/react/web`.
 *
 * Ordering: listeners forward in DOM event order and the sink must
 * preserve that order into one session-fenced ingress stream (the
 * correlated production `RequestBody::GuiInput` path via
 * {@link createGuiInputSink}, or `WorldContext::enqueue_gui_input_command`
 * for host-owned loops). One active input context per World supports
 * multiple panels and pointer identities; other clients may author or
 * observe without acquiring input ownership.
 */

export type GuiPointerButton = "primary" | "secondary" | "auxiliary";

export type GuiKey =
  | "tab"
  | "enter"
  | "space"
  | "escape"
  | "backspace"
  | "delete"
  | "left"
  | "right"
  | "up"
  | "down"
  | "home"
  | "end";

/** GUI logical point: top-left origin, +X right, +Y down. */
export type GuiLogicalPoint = readonly [number, number];

/**
 * Normalized viewport point: CSS fraction of the live canvas rect,
 * top-left origin, +Y down. DevicePixelRatio cancels in the normalization,
 * so the same on-screen point maps identically across densities; values
 * may extend outside `0..=1`, matching the geometry pick convention. The
 * core projects these through the current camera, so scene-fallback sinks
 * (including `onUnhandled`) reuse this same point for their own rays.
 */
export type GuiViewportPoint = readonly [number, number];

/**
 * Explicitly marked scene blocker. The distance rides along for logical
 * routing without a camera; on the projected path the core resolves every
 * marked distance from current-tick scene geometry instead.
 */
export interface GuiBlockerHit {
  readonly entity: bigint;
  readonly distance: number;
}

/** Browser-shaped GUI input, mirroring the core `GuiInputCommand`. */
export type BrowserGuiInputCommand =
  | {
      readonly kind: "pointerDown";
      readonly pointer: number;
      readonly position: GuiViewportPoint;
      readonly button: GuiPointerButton;
      readonly blockers?: readonly GuiBlockerHit[];
      readonly panelDistance?: number;
    }
  | {
      readonly kind: "pointerUp";
      readonly pointer: number;
      readonly position: GuiViewportPoint;
      readonly button: GuiPointerButton;
      readonly blockers?: readonly GuiBlockerHit[];
      readonly panelDistance?: number;
    }
  | {
      readonly kind: "pointerMove";
      readonly pointer: number;
      readonly position: GuiViewportPoint;
      readonly blockers?: readonly GuiBlockerHit[];
      readonly panelDistance?: number;
    }
  | { readonly kind: "pointerCancel"; readonly pointer: number }
  | {
      readonly kind: "scroll";
      readonly position: GuiViewportPoint;
      readonly delta: GuiLogicalPoint;
      readonly blockers?: readonly GuiBlockerHit[];
      readonly panelDistance?: number;
    }
  | { readonly kind: "key"; readonly key: GuiKey; readonly pressed: true }
  | { readonly kind: "text"; readonly text: string }
  | { readonly kind: "blur" }
  | {
      readonly kind: "composition";
      readonly text: string;
      readonly caretStart: number;
      readonly caretEnd: number;
    }
  | { readonly kind: "commitComposition" }
  | { readonly kind: "cancelComposition" }
  | {
      readonly kind: "selection";
      readonly start: number;
      readonly end: number;
    };

/** Ordered, session-fenced ingress sink. Must preserve call order. */
export interface GuiInputSink {
  send(command: BrowserGuiInputCommand): void;
  /**
   * Revoke the sink after detach: later sends drop instead of reaching a
   * replacement session. Optional so older sinks keep compiling; the canvas
   * relay calls it when present.
   */
  close?(): void;
}

/** Minimal submitter surface the browser sink needs from a GUI-capable client. */
export interface GuiInputSubmitter {
  submitGuiInput(input: GuiInputCommand): Promise<GuiInputRoutingOutcome>;
}

export interface GuiInputSinkOptions {
  readonly onError?: (error: Error) => void;
  /** Route only authoritative no-panel misses into scene gesture admission. */
  readonly unhandledInputGate?: GuiUnhandledInputGate;
}

/** Translate one relayed browser command to the wire input command.
 *
 * Positions already arrive as normalized viewport points; the relay
 * carries no panel scope, so callers needing a non-overlay panel supply
 * `panel` at the wire layer instead.
 */
export function toGuiInputCommand(
  command: BrowserGuiInputCommand,
): GuiInputCommand {
  switch (command.kind) {
    case "pointerDown":
      return {
        kind: "pointerDown",
        pointer: command.pointer,
        position: [command.position[0], command.position[1]],
        button: command.button,
        ...(command.blockers === undefined
          ? {}
          : { blockers: command.blockers }),
        ...(command.panelDistance === undefined
          ? {}
          : { panelDistance: command.panelDistance }),
      };
    case "pointerUp":
      return {
        kind: "pointerUp",
        pointer: command.pointer,
        position: [command.position[0], command.position[1]],
        button: command.button,
        ...(command.blockers === undefined
          ? {}
          : { blockers: command.blockers }),
        ...(command.panelDistance === undefined
          ? {}
          : { panelDistance: command.panelDistance }),
      };
    case "pointerMove":
      return {
        kind: "pointerMove",
        pointer: command.pointer,
        position: [command.position[0], command.position[1]],
        ...(command.blockers === undefined
          ? {}
          : { blockers: command.blockers }),
        ...(command.panelDistance === undefined
          ? {}
          : { panelDistance: command.panelDistance }),
      };
    case "pointerCancel":
      return { kind: "pointerCancel", pointer: command.pointer };
    case "scroll":
      return {
        kind: "scroll",
        position: [command.position[0], command.position[1]],
        delta: [command.delta[0], command.delta[1]],
        ...(command.blockers === undefined
          ? {}
          : { blockers: command.blockers }),
        ...(command.panelDistance === undefined
          ? {}
          : { panelDistance: command.panelDistance }),
      };
    case "key":
      return { kind: "key", key: command.key, pressed: true };
    case "text":
      return { kind: "text", text: command.text };
    case "blur":
      return { kind: "blur" };
    case "composition":
      return {
        kind: "composition",
        text: command.text,
        caretStart: command.caretStart,
        caretEnd: command.caretEnd,
      };
    case "commitComposition":
      return { kind: "commitComposition" };
    case "cancelComposition":
      return { kind: "cancelComposition" };
    case "selection":
      return {
        kind: "setTextSelection",
        start: command.start,
        end: command.end,
      };
  }
}

/** Map relayed browser commands onto a session-fenced ordered submitter.
 *
 * Every command is submitted immediately in call order. The transport and
 * Host preserve that order; replies report each command independently and
 * never gate later input. Every rejection reaches `onError`.
 */
export function createGuiInputSink(
  submitter: GuiInputSubmitter,
  options: GuiInputSinkOptions = {},
): GuiInputSink {
  const { onError, unhandledInputGate } = options;
  const report = (error: unknown): void => {
    onError?.(error instanceof Error ? error : new Error(String(error)));
  };
  const gateGeneration = openUnhandledInputGate(unhandledInputGate);
  let closed = false;

  const submit = (command: BrowserGuiInputCommand): void => {
    const gateSubmission = trackUnhandledInputGate(
      unhandledInputGate,
      gateGeneration,
      command,
    );
    let input: GuiInputCommand;
    try {
      input = toGuiInputCommand(command);
    } catch (error) {
      settleUnhandledInputGateSubmission(unhandledInputGate, gateSubmission);
      report(error);
      return;
    }
    let request: Promise<GuiInputRoutingOutcome>;
    try {
      request = submitter.submitGuiInput(input);
    } catch (error) {
      settleUnhandledInputGateSubmission(unhandledInputGate, gateSubmission);
      report(error);
      return;
    }
    void request.then(
      (outcome) => {
        settleUnhandledInputGateSubmission(
          unhandledInputGate,
          gateSubmission,
          outcome,
        );
      },
      (error: unknown) => {
        settleUnhandledInputGateSubmission(unhandledInputGate, gateSubmission);
        report(error);
      },
    );
  };

  return {
    send(command: BrowserGuiInputCommand): void {
      // Fence queued relay sends to the owning context: after close, late
      // input drops instead of entering a replacement session.
      if (closed) return;
      submit(command);
    },
    close(): void {
      closed = true;
      closeUnhandledInputGate(unhandledInputGate, gateGeneration);
    },
  };
}

export interface AttachCanvasGuiInputOptions {
  /**
   * Remap a normalized viewport point (identity by default) for
   * letterboxed canvases. This replaces the old CSS-to-logical hook:
   * pointer positions now ship normalized so the core can project them
   * through the current camera.
   */
  readonly toViewport?: (point: GuiViewportPoint) => GuiViewportPoint;
  /** Explicitly marked scene blockers; visual occlusion alone never blocks. */
  readonly blockers?: readonly GuiBlockerHit[];
  /**
   * World-space panel distance; omitted means overlay-nearest on logical
   * routing. On the projected path the core resolves panel distances
   * from the camera ray and this stays an unused hint.
   */
  readonly panelDistance?: number;
  /** Element receiving keyboard/text input; defaults to the canvas. */
  readonly keyboardTarget?: HTMLElement | null;
  /**
   * Whether this relay owns keyboard, text and composition listeners on the
   * keyboard target. Disable when an adapter-owned native editor is the sole
   * translator for that event stream; pointer, wheel and window-blur routing
   * remain attached.
   */
  readonly keyboardInput?: boolean;
  /**
   * Send core blur when `keyboardTarget` loses DOM focus. Disable this when
   * the target is an adapter-owned native editor whose focus follows core
   * focus; window blur still clears the input context.
   */
  readonly blurOnKeyboardTarget?: boolean;
  /** Call `preventDefault` on pointer events before async responses. */
  readonly preventDefaultPointer?: boolean;
  /** Set `touch-action: none` while attached; restored on detach. */
  readonly enableTouchActionNone?: boolean;
  /**
   * Mount the native editable buffer beside the canvas for IME and
   * soft-keyboard input; pass `false` to keep the canvas-only relay.
   * Mounted by default; see `attachTextBridge`.
   */
  readonly textBridge?: boolean;
  /** Optional shared gate for camera or other scene fallback gestures. */
  readonly unhandledInputGate?: GuiUnhandledInputGate;
  readonly onError?: (error: Error) => void;
}

/** DOM mouse button to GUI button; other buttons are ignored. */
export function domMouseButtonToGuiButton(
  button: number,
): GuiPointerButton | null {
  switch (button) {
    case 0:
      return "primary";
    case 2:
      return "secondary";
    case 1:
      return "auxiliary";
    default:
      return null;
  }
}

/** `KeyboardEvent.key` to non-text GUI key; printable input uses `text`. */
export function keyboardKeyToGuiKey(key: string): GuiKey | null {
  switch (key) {
    case "Tab":
      return "tab";
    case "Enter":
      return "enter";
    case " ":
    case "Spacebar":
      return "space";
    case "Escape":
      return "escape";
    case "Backspace":
      return "backspace";
    case "Delete":
      return "delete";
    case "ArrowLeft":
      return "left";
    case "ArrowRight":
      return "right";
    case "ArrowUp":
      return "up";
    case "ArrowDown":
      return "down";
    case "Home":
      return "home";
    case "End":
      return "end";
    default:
      return null;
  }
}

/** Client coordinates to canvas-relative CSS pixels. */
export function canvasRelativePoint(
  clientX: number,
  clientY: number,
  rect: { readonly left: number; readonly top: number },
): GuiLogicalPoint {
  return [clientX - rect.left, clientY - rect.top];
}

/**
 * Client coordinates to a normalized viewport point for the live canvas
 * rect. Returns null when the rect has no area, so callers skip events
 * that carry no viewport. Scene-fallback sinks reuse this same point for
 * their own rays; it is all routing needs beyond the ordered commands.
 */
export function canvasViewportPoint(
  clientX: number,
  clientY: number,
  rect: {
    readonly left: number;
    readonly top: number;
    readonly width: number;
    readonly height: number;
  },
): GuiViewportPoint | null {
  if (
    !Number.isFinite(rect.width) ||
    !Number.isFinite(rect.height) ||
    rect.width <= 0 ||
    rect.height <= 0
  ) {
    return null;
  }
  return [
    (clientX - rect.left) / rect.width,
    (clientY - rect.top) / rect.height,
  ];
}

/** Wheel deltas to logical units; line scrolls assume 16 px per line. */
export function wheelDeltaToLogical(
  deltaX: number,
  deltaY: number,
  deltaMode: number,
): GuiLogicalPoint {
  if (deltaMode === 1) return [deltaX * 16, deltaY * 16];
  return [deltaX, deltaY];
}

function toU32(pointerId: number): number {
  return pointerId >>> 0;
}

/** Attach real browser input to an ordered GUI ingress sink.
 *
 * Feeds the existing ordered ingress: pointer (mouse/touch/pen via
 * Pointer Events), wheel-as-scroll, keyboard keys, `beforeinput` text,
 * IME composition updates/commits, and platform blur. Capture, focus
 * scopes and gesture arbitration stay in core; releases of other buttons
 * never complete a press there. Composition `beforeinput` payloads are
 * skipped so each provisional applies exactly once.
 *
 * Browser capture management: presses acquire canvas pointer capture so a
 * drag released outside the canvas still terminates exactly once on the
 * canvas; capture loss, window blur and detach each cancel live pointers
 * and clear the input context before access is lost. Detach revokes the
 * sink, so queued relay sends never enter a replacement session. Returns
 * a detach function removing every listener.
 */
export function attachCanvasGuiInput(
  canvas: HTMLCanvasElement,
  sink: GuiInputSink,
  options: AttachCanvasGuiInputOptions = {},
): () => void {
  const {
    toViewport = (point) => point,
    blockers = [],
    panelDistance,
    keyboardTarget = null,
    keyboardInput = true,
    blurOnKeyboardTarget = true,
    preventDefaultPointer = false,
    enableTouchActionNone = true,
    onError,
  } = options;
  const report = (error: unknown): void => {
    onError?.(error instanceof Error ? error : new Error(String(error)));
  };
  // Live pointers by relay identity, keeping the raw browser identity for
  // capture calls: the relay converts to u32 while the browser API takes
  // the event's own identifier.
  const live = new Map<number, number>();
  const target = keyboardTarget ?? canvas;
  let detached = false;
  // `exactOptionalPropertyTypes`: omit when unset rather than assigning
  // `undefined` explicitly.
  const maybeDistance = panelDistance === undefined ? {} : { panelDistance };

  const viewportAt = (event: PointerEvent): GuiViewportPoint | null => {
    const rect = canvas.getBoundingClientRect();
    const point = canvasViewportPoint(event.clientX, event.clientY, rect);
    if (point === null) return null;
    return toViewport(point);
  };

  const send = (command: BrowserGuiInputCommand): void => {
    if (detached) return;
    try {
      sink.send(command);
    } catch (error) {
      report(error);
    }
  };

  /** Acquire canvas capture for one press; drags outside the canvas then
   * retarget to it instead of losing their release. Missing capture APIs
   * (headless stubs) keep the old listener-only behavior. */
  const acquireCapture = (pointerId: number): void => {
    try {
      canvas.setPointerCapture?.(pointerId);
    } catch (error) {
      report(error);
    }
  };

  /** Release canvas capture when still held; never throws out. */
  const releaseCapture = (pointerId: number): void => {
    try {
      if (canvas.hasPointerCapture?.(pointerId) ?? false) {
        canvas.releasePointerCapture?.(pointerId);
      }
    } catch (error) {
      report(error);
    }
  };

  /** Terminate one pointer exactly once: later up/cancel/loss events for
   * the same identity find no live entry and send nothing. */
  const terminate = (
    pointer: number,
    command: BrowserGuiInputCommand,
  ): void => {
    const raw = live.get(pointer);
    if (raw === undefined) return;
    live.delete(pointer);
    releaseCapture(raw);
    send(command);
  };

  const onPointerDown = (event: PointerEvent): void => {
    const button = domMouseButtonToGuiButton(event.button);
    if (button === null) return;
    const position = viewportAt(event);
    if (position === null) return;
    if (preventDefaultPointer) event.preventDefault();
    const pointer = toU32(event.pointerId);
    live.set(pointer, event.pointerId);
    acquireCapture(event.pointerId);
    send({
      kind: "pointerDown",
      pointer,
      position,
      button,
      blockers,
      ...maybeDistance,
    });
  };

  const onPointerMove = (event: PointerEvent): void => {
    const position = viewportAt(event);
    if (position === null) return;
    send({
      kind: "pointerMove",
      pointer: toU32(event.pointerId),
      position,
      blockers,
      ...maybeDistance,
    });
  };

  const onPointerUp = (event: PointerEvent): void => {
    const button = domMouseButtonToGuiButton(event.button);
    if (button === null) return;
    const position = viewportAt(event);
    if (position === null) return;
    const pointer = toU32(event.pointerId);
    terminate(pointer, {
      kind: "pointerUp",
      pointer,
      position,
      button,
      blockers,
      ...maybeDistance,
    });
  };

  const onPointerCancel = (event: PointerEvent): void => {
    const pointer = toU32(event.pointerId);
    terminate(pointer, { kind: "pointerCancel", pointer });
  };

  /** The browser took capture without a release (focus loss, overlay, or
   * explicit takeover): cancel the live press so no runtime capture stays
   * active. A normal release already terminated the pointer, so the
   * post-release loss event sends nothing. */
  const onLostPointerCapture = (event: PointerEvent): void => {
    const pointer = toU32(event.pointerId);
    terminate(pointer, { kind: "pointerCancel", pointer });
  };

  const onWheel = (event: WheelEvent): void => {
    event.preventDefault();
    const rect = canvas.getBoundingClientRect();
    const raw = canvasViewportPoint(event.clientX, event.clientY, rect);
    if (raw === null) return;
    send({
      kind: "scroll",
      position: toViewport(raw),
      delta: wheelDeltaToLogical(event.deltaX, event.deltaY, event.deltaMode),
      blockers,
      ...maybeDistance,
    });
  };

  const onKeyDown = (event: KeyboardEvent): void => {
    const key = keyboardKeyToGuiKey(event.key);
    if (key === null) return;
    if (key === "tab") event.preventDefault();
    send({ kind: "key", key, pressed: true });
  };

  const ime = createImeBridge(
    { send },
    onError === undefined ? {} : { onError },
  );

  const onCompositionStart = (): void => {
    ime.compositionStart();
  };

  const onBeforeInput = (event: InputEvent): void => {
    if (typeof event.data !== "string" || event.data.length === 0) return;
    if (event.inputType.startsWith("delete")) return;
    if (shouldSkipBeforeInput(event.inputType)) return;
    send({ kind: "text", text: event.data });
  };

  const onCompositionUpdate = (event: CompositionEvent): void => {
    ime.compositionUpdate(event.data);
  };

  const onCompositionEnd = (event: CompositionEvent): void => {
    ime.compositionEnd(event.data);
  };

  const onBlur = (): void => {
    for (const pointer of [...live.keys()]) {
      terminate(pointer, { kind: "pointerCancel", pointer });
    }
    ime.blur();
    send({ kind: "blur" });
  };

  canvas.addEventListener("pointerdown", onPointerDown);
  canvas.addEventListener("pointermove", onPointerMove);
  canvas.addEventListener("pointerup", onPointerUp);
  canvas.addEventListener("pointercancel", onPointerCancel);
  canvas.addEventListener(
    "lostpointercapture",
    onLostPointerCapture as EventListener,
  );
  canvas.addEventListener("wheel", onWheel, { passive: false });
  if (keyboardInput) {
    target.addEventListener("keydown", onKeyDown as EventListener);
    target.addEventListener("beforeinput", onBeforeInput as EventListener);
    target.addEventListener(
      "compositionstart",
      onCompositionStart as EventListener,
    );
    target.addEventListener(
      "compositionupdate",
      onCompositionUpdate as EventListener,
    );
    target.addEventListener(
      "compositionend",
      onCompositionEnd as EventListener,
    );
    if (blurOnKeyboardTarget) target.addEventListener("blur", onBlur);
  }
  // Window blur fires when the canvas target never blurs itself (alt-tab,
  // devtools focus); headless runtimes skip it without a window.
  const windowTarget = typeof window !== "undefined" ? window : null;
  windowTarget?.addEventListener("blur", onBlur);

  const previousTouchAction = canvas.style.touchAction;
  if (enableTouchActionNone) canvas.style.touchAction = "none";

  return () => {
    // Detach cancels the context it owns before losing access: every live
    // press terminates and the focus clears, then the sink revokes so late
    // events never enter a replacement session (canvas replacement,
    // unmount-while-held, connection/session changes).
    for (const pointer of [...live.keys()]) {
      terminate(pointer, { kind: "pointerCancel", pointer });
    }
    ime.blur();
    send({ kind: "blur" });
    detached = true;
    sink.close?.();
    canvas.removeEventListener("pointerdown", onPointerDown);
    canvas.removeEventListener("pointermove", onPointerMove);
    canvas.removeEventListener("pointerup", onPointerUp);
    canvas.removeEventListener("pointercancel", onPointerCancel);
    canvas.removeEventListener(
      "lostpointercapture",
      onLostPointerCapture as EventListener,
    );
    canvas.removeEventListener("wheel", onWheel);
    if (keyboardInput) {
      target.removeEventListener("keydown", onKeyDown as EventListener);
      target.removeEventListener("beforeinput", onBeforeInput as EventListener);
      target.removeEventListener(
        "compositionstart",
        onCompositionStart as EventListener,
      );
      target.removeEventListener(
        "compositionupdate",
        onCompositionUpdate as EventListener,
      );
      target.removeEventListener(
        "compositionend",
        onCompositionEnd as EventListener,
      );
      if (blurOnKeyboardTarget) target.removeEventListener("blur", onBlur);
    }
    windowTarget?.removeEventListener("blur", onBlur);
    if (enableTouchActionNone) canvas.style.touchAction = previousTouchAction;
  };
}
