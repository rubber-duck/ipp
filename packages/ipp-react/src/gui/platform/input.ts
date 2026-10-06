import type {
  GuiPhysicalContext,
  GuiPhysicalInput,
  GuiPhysicalKey,
  GuiInputRoutingOutcome,
  HostPhysicalInput,
  PresentationView,
  GuiNativeEdit,
  GuiTextFence,
  GuiPhysicalContextOptions,
} from "@ipp/client";
import {
  attachTextBridge,
  createTextBridgeModel,
  viewportToBridgeOffset,
} from "./text-bridge.js";
import {
  openUnhandledInputGate,
  closeUnhandledInputGate,
  trackUnhandledInputGate,
  settleUnhandledInputGateSubmission,
  type GuiUnhandledInputGate,
} from "./scene-input.js";
import { keyboardKeyToGuiKey } from "./keys.js";

export type GuiViewportPoint = readonly [number, number];
export type BrowserGuiInputCommand = (
  | GuiNativeEdit
  | { kind: "key"; key: GuiPhysicalKey; shift?: boolean }
  | { kind: "blur" }
) & { readonly fence?: GuiTextFence };
export interface GuiInputSink {
  send(command: BrowserGuiInputCommand): void;
}

/** Physical keys sent as routed input rather than native text edits. */
const routedKeys: ReadonlySet<string> = new Set([
  "tab",
  "backTab",
  "escape",
  "up",
  "down",
  "space",
  "contextMenu",
  "f10",
]);

export interface CanvasGuiInputOptions extends GuiPhysicalContextOptions {
  readonly unhandledInputGate?: GuiUnhandledInputGate;
  /**
   * GUI logical units one wheel notch scrolls; finite and positive,
   * {@link DEFAULT_GUI_WHEEL_STEP} by default or when invalid. See
   * {@link wheelDeltaToLogical} for choosing it.
   */
  readonly wheelStep?: number;
  readonly onError: (error: Error) => void;
  readonly onUnhandled?: (
    input: GuiPhysicalInput,
    outcome: GuiInputRoutingOutcome,
  ) => void;
}

/**
 * Default GUI logical units one wheel notch scrolls: an eighth of a 2-unit
 * ScrollView viewport.
 */
export const DEFAULT_GUI_WHEEL_STEP = 0.25;

/** CSS pixels one wheel notch reports in pixel mode (Chromium and Windows). */
const WHEEL_NOTCH_PIXELS = 100;

/** Lines one wheel notch reports in line mode (Firefox). */
const WHEEL_NOTCH_LINES = 3;

/** Notches one page counts in page mode: one viewport at the recommended
 * step of an eighth of the viewport per notch. */
const WHEEL_PAGE_NOTCHES = 8;

/**
 * Browser wheel deltas to GUI logical units.
 *
 * Browser deltas count CSS pixels, lines or pages, none of which relate to a
 * canvas's logical units: canvases are authored at any density and seen at any
 * projected size. Deltas therefore convert to wheel notches first, with
 * fractional notches for smooth pixel scrolling (trackpads), and each notch
 * scrolls `step` logical units. Choose the step as a fraction of the
 * ScrollView viewports the canvas shows, about an eighth of the smallest.
 */
export function wheelDeltaToLogical(
  deltaX: number,
  deltaY: number,
  deltaMode: number,
  step = DEFAULT_GUI_WHEEL_STEP,
): readonly [number, number] {
  const notches = (delta: number): number =>
    deltaMode === WheelEvent.DOM_DELTA_LINE
      ? delta / WHEEL_NOTCH_LINES
      : deltaMode === WheelEvent.DOM_DELTA_PAGE
        ? delta * WHEEL_PAGE_NOTCHES
        : delta / WHEEL_NOTCH_PIXELS;
  return [notches(deltaX) * step, notches(deltaY) * step];
}

/** DOM order enters one Host-owned physical context. Local capture only keeps
 * out-of-bounds releases observable; callbacks use committed effect observations.
 */
export function attachCanvasGuiInput(
  canvas: HTMLCanvasElement,
  context: GuiPhysicalContext,
  options: CanvasGuiInputOptions,
): () => void {
  let live = true;
  const gateGeneration = openUnhandledInputGate(options.unhandledInputGate);
  const model = createTextBridgeModel(context.identity);
  let epoch = 0;
  let working = false;
  const nativePending: { command: BrowserGuiInputCommand; epoch: number }[] =
    [];
  const report = (error: unknown) =>
    options.onError(error instanceof Error ? error : new Error(String(error)));
  // An invalid step is reported once and scrolls by the default instead.
  let wheelStep = options.wheelStep ?? DEFAULT_GUI_WHEEL_STEP;
  if (!Number.isFinite(wheelStep) || wheelStep <= 0) {
    report(
      new RangeError(
        `GUI wheel step must be finite and positive, not ${wheelStep}`,
      ),
    );
    wheelStep = DEFAULT_GUI_WHEEL_STEP;
  }
  const drainNative = async () => {
    if (working) return;
    working = true;
    try {
      while (live && !context.isClosed && nativePending.length) {
        const entry = nativePending.shift()!;
        if (entry.epoch !== epoch) continue;
        const command = entry.command;
        if (
          command.kind === "blur" ||
          (command.kind === "key" && routedKeys.has(command.key))
        ) {
          await context.send(command as GuiPhysicalInput);
          continue;
        }
        const state = context.nativeText;
        if (!state) continue;
        const outcome = await context.editText(
          command.fence ?? state.fence,
          command as GuiNativeEdit,
        );
        if (outcome.rejected || outcome.cancelled || outcome.error) {
          nativePending.length = 0;
          report(new Error(outcome.error ?? "Native text edit cancelled"));
        }
      }
    } catch (error) {
      nativePending.length = 0;
      if (live) report(error);
    } finally {
      working = false;
    }
  };
  const sink: GuiInputSink = {
    send(command) {
      if (!live || context.isClosed) return;
      if (nativePending.length >= 32)
        throw new Error("Native text ingress capacity exceeded");
      nativePending.push({ command, epoch });
      void drainNative();
    },
  };
  const bridge = attachTextBridge(canvas.parentElement ?? document.body, sink, {
    onError: report,
    getFocusToken: model.token,
    readCommitted: model.committed,
    onLocalSelection: (selection) =>
      model.noteLocalSelection(selection.start, selection.end),
  });
  const textSubscription = context.onText((state, origin) => {
    if (origin === "notification") {
      epoch += 1;
      nativePending.length = 0;
      bridge.invalidate();
    }
    const ownedFocus = document.activeElement === bridge.element;
    model.observe(state);
    bridge.syncFromCore();
    if (state && document.hasFocus())
      bridge.element.focus({ preventScroll: true });
    else if (ownedFocus && !context.isClosed && document.hasFocus())
      canvas.focus({ preventScroll: true });
  });
  const captures = new Set<number>();
  const originalTabIndex = canvas.getAttribute("tabindex");
  if (originalTabIndex === null) canvas.tabIndex = 0;
  const originalTouchAction = canvas.style.touchAction;
  canvas.style.touchAction = "none";
  const point = (event: MouseEvent): readonly [number, number] => {
    const rect = canvas.getBoundingClientRect();
    return [
      (event.clientX - rect.left) / rect.width,
      (event.clientY - rect.top) / rect.height,
    ];
  };
  const release = (pointer: number) => {
    captures.delete(pointer);
    if (canvas.hasPointerCapture(pointer))
      canvas.releasePointerCapture(pointer);
  };
  const send = (input: GuiPhysicalInput) => {
    if (!live || context.isClosed) return;
    const submission = trackUnhandledInputGate(
      options.unhandledInputGate,
      gateGeneration,
      input,
    );
    void context
      .send(input)
      .then((outcome) => {
        settleUnhandledInputGateSubmission(
          options.unhandledInputGate,
          submission,
          outcome,
        );
        if (!live) return;
        if (
          outcome.rejected ||
          outcome.cancelled ||
          outcome.error !== undefined
        ) {
          for (const pointer of [...captures]) release(pointer);
          return;
        }
        if (input.kind === "pointerDown" && outcome.disposition !== "routed")
          release(Number(input.pointer));
        if (
          input.kind === "wheel" &&
          outcome.remaining?.some((value) => value !== 0)
        )
          options.onUnhandled?.(
            { ...input, delta: outcome.remaining },
            outcome,
          );
        else if (
          outcome.disposition === "miss" ||
          outcome.disposition === "unhandled"
        )
          options.onUnhandled?.(input, outcome);
      })
      .catch((error: unknown) => {
        settleUnhandledInputGateSubmission(
          options.unhandledInputGate,
          submission,
        );
        for (const pointer of [...captures]) release(pointer);
        if (live)
          options.onError(
            error instanceof Error ? error : new Error(String(error)),
          );
      });
  };
  const down = (event: PointerEvent) => {
    if (!live || event.button < 0 || event.button > 2) return;
    if (event.button !== 0) {
      event.preventDefault();
      send({
        kind: "pointerDown",
        pointer: BigInt(event.pointerId),
        point: point(event),
        button: event.button === 1 ? "auxiliary" : "secondary",
      });
      return;
    }
    if (!context.nativeText) canvas.focus({ preventScroll: true });
    epoch += 1;
    nativePending.length = 0;
    model.noteActivation();
    captures.add(event.pointerId);
    canvas.setPointerCapture(event.pointerId);
    send({
      kind: "pointerDown",
      pointer: BigInt(event.pointerId),
      point: point(event),
    });
    event.preventDefault();
  };
  const move = (event: PointerEvent) =>
    send({
      kind: "pointerMove",
      pointer: BigInt(event.pointerId),
      point: point(event),
    });
  const up = (event: PointerEvent) => {
    if (event.button < 0 || event.button > 2) return;
    if (event.button !== 0) {
      send({
        kind: "pointerUp",
        pointer: BigInt(event.pointerId),
        point: point(event),
        button: event.button === 1 ? "auxiliary" : "secondary",
      });
      return;
    }
    send({
      kind: "pointerUp",
      pointer: BigInt(event.pointerId),
      point: point(event),
    });
    release(event.pointerId);
    if (context.nativeText) {
      const container = canvas.parentElement ?? document.body;
      const offset = viewportToBridgeOffset(
        point(event),
        canvas.getBoundingClientRect(),
        container.getBoundingClientRect(),
      );
      bridge.placeAt(offset.x, offset.y);
      bridge.focusFromGesture({ trigger: "tap", isTrusted: event.isTrusted });
    }
  };
  const cancel = (event: PointerEvent) => {
    if (!captures.has(event.pointerId)) return;
    send({ kind: "pointerCancel", pointer: BigInt(event.pointerId) });
    release(event.pointerId);
  };
  const wheel = (event: WheelEvent) => {
    const [x, y] = wheelDeltaToLogical(
      event.deltaX,
      event.deltaY,
      event.deltaMode,
      wheelStep,
    );
    send({
      kind: "wheel",
      point: point(event),
      delta: [x, y],
      ...(event.shiftKey ? { shift: true } : {}),
    });
    event.preventDefault();
  };
  const key = (event: KeyboardEvent) => {
    if (event.isComposing || event.ctrlKey || event.metaKey || event.altKey)
      return;
    const key = keyboardKeyToGuiKey(event.key, event.shiftKey);
    if (!key || key === "backspace" || key === "delete") return;
    send({ kind: "key", key, ...(event.shiftKey ? { shift: true } : {}) });
    event.preventDefault();
  };
  // Context requests go to the runtime, so the browser's own menu never opens
  // over the canvas or its native text buffer; the decision cannot wait for
  // the asynchronous routing outcome.
  const contextMenu = (event: Event) => event.preventDefault();
  const blur = () => {
    send({ kind: "blur" });
    for (const pointer of [...captures]) release(pointer);
  };
  const canvasBlur = (event: FocusEvent) => {
    if (event.relatedTarget !== bridge.element) blur();
  };
  const bridgeKey = (event: KeyboardEvent) => {
    if (event.key === "Tab") {
      event.preventDefault();
      if (context.nativeText === null) canvas.focus({ preventScroll: true });
    }
  };
  bridge.element.addEventListener("keydown", bridgeKey);
  bridge.element.addEventListener("contextmenu", contextMenu);
  canvas.addEventListener("contextmenu", contextMenu);
  canvas.addEventListener("pointerdown", down);
  canvas.addEventListener("pointermove", move);
  canvas.addEventListener("pointerup", up);
  canvas.addEventListener("pointercancel", cancel);
  canvas.addEventListener("lostpointercapture", cancel);
  canvas.addEventListener("wheel", wheel, { passive: false });
  canvas.addEventListener("keydown", key);
  canvas.addEventListener("blur", canvasBlur);
  window.addEventListener("blur", blur);
  const detach = () => {
    if (!live) return;
    live = false;
    closeUnhandledInputGate(options.unhandledInputGate, gateGeneration);
    epoch += 1;
    nativePending.length = 0;
    textSubscription();
    bridge.element.removeEventListener("keydown", bridgeKey);
    bridge.element.removeEventListener("contextmenu", contextMenu);
    canvas.removeEventListener("contextmenu", contextMenu);
    bridge.dispose();
    for (const pointer of [...captures]) release(pointer);
    canvas.removeEventListener("pointerdown", down);
    canvas.removeEventListener("pointermove", move);
    canvas.removeEventListener("pointerup", up);
    canvas.removeEventListener("pointercancel", cancel);
    canvas.removeEventListener("lostpointercapture", cancel);
    canvas.removeEventListener("wheel", wheel);
    canvas.removeEventListener("keydown", key);
    canvas.removeEventListener("blur", canvasBlur);
    window.removeEventListener("blur", blur);
    if (originalTabIndex === null) canvas.removeAttribute("tabindex");
    else canvas.setAttribute("tabindex", originalTabIndex);
    canvas.style.touchAction = originalTouchAction;
  };
  const unsubscribe = context.onClose(detach);
  const cancelNative = context.onCancel((event) => {
    for (const pointer of event.pointers) release(Number(pointer));
  });
  return () => {
    unsubscribe();
    cancelNative();
    detach();
  };
}

/** Snapshot the configuration retained by a physical context and DOM listeners. */
function inputConfiguration(
  options: CanvasGuiInputOptions,
): CanvasGuiInputOptions {
  return {
    ...options,
    ...(options.blockers
      ? {
          blockers: options.blockers.map((blocker) => ({
            ...blocker,
            world: { ...blocker.world },
          })),
        }
      : {}),
  };
}

function sameInputConfiguration(
  left: CanvasGuiInputOptions,
  right: CanvasGuiInputOptions,
): boolean {
  const identities = (options: CanvasGuiInputOptions) =>
    (options.blockers ?? [])
      .map(
        ({ world, entity, incarnation }) =>
          `${world.id}:${world.incarnation}:${entity}:${incarnation}`,
      )
      .sort();
  const a = identities(left);
  const b = identities(right);
  return (
    Object.is(
      left.wheelStep ?? DEFAULT_GUI_WHEEL_STEP,
      right.wheelStep ?? DEFAULT_GUI_WHEEL_STEP,
    ) &&
    left.unhandledInputGate === right.unhandledInputGate &&
    a.length === b.length &&
    a.every((value, index) => value === b[index])
  );
}

/** A physical view lifetime, independent of authoring sessions. */
export class CanvasGuiInput {
  private generation = 0;
  private context: GuiPhysicalContext | undefined;
  private readonly retiring = new Set<GuiPhysicalContext>();
  private detach: (() => void) | undefined;
  private stopped = false;
  private view: PresentationView | null = null;
  private pending: Promise<void> = Promise.resolve();
  private configuration: CanvasGuiInputOptions;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly input: HostPhysicalInput,
    private options: CanvasGuiInputOptions,
  ) {
    this.configuration = inputConfiguration(options);
  }

  /** Callback updates retain capture; physical configuration changes release it. */
  update(options: CanvasGuiInputOptions): Promise<void> {
    if (this.stopped) return this.pending;
    this.options = options;
    const configuration = inputConfiguration(options);
    if (sameInputConfiguration(this.configuration, configuration))
      return this.pending;
    this.configuration = configuration;
    return this.select(this.view);
  }

  select(view: PresentationView | null): Promise<void> {
    this.view = this.stopped ? null : view;
    const generation = ++this.generation;
    const configuration = this.configuration;
    const selected = this.view;
    if (this.context) this.retiring.add(this.context);
    this.context = undefined;
    this.detach?.();
    this.detach = undefined;
    // Serialize acquisitions and releases: a late open must close before a new
    // one can own the same presented root. Detach native capture immediately.
    const work = this.pending.then(async () => {
      await this.releaseRetiring();
      if (!selected || generation !== this.generation || this.stopped) return;
      const context = await this.input.open(selected, configuration);
      if (generation !== this.generation || this.stopped) {
        this.retiring.add(context);
        await this.releaseRetiring();
        return;
      }
      this.context = context;
      try {
        this.detach = attachCanvasGuiInput(this.canvas, context, {
          ...configuration,
          onError: (error) => this.options.onError(error),
          onUnhandled: (input, outcome) =>
            this.options.onUnhandled?.(input, outcome),
        });
      } catch (error) {
        this.context = undefined;
        this.retiring.add(context);
        await this.releaseRetiring();
        throw error;
      }
    });
    // Keep later cleanup runnable after an earlier failed acquisition.
    this.pending = work.catch(() => {});
    return work;
  }

  private async releaseRetiring(): Promise<void> {
    for (const context of this.retiring) {
      await context.close();
      // A request not sent may be retried by the next select/close. Retain its
      // ownership until release succeeds so acquisition cannot overtake it.
      this.retiring.delete(context);
    }
  }

  close(): Promise<void> {
    this.stopped = true;
    return this.select(null);
  }
}
