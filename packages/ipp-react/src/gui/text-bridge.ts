import type { GuiObservationBatch, GuiTextFocusState } from "@ipp/client";
import type { GuiInputSink, GuiViewportPoint } from "./input.js";
import { keyboardKeyToGuiKey } from "./input.js";
import {
  createImeBridge,
  shouldSkipBeforeInput,
  utf8ByteLength,
  type ImeBridge,
} from "./ime.js";
import {
  CLIPBOARD_TEXT_MAX_BYTES,
  copyFocusedTextToClipboard,
  pasteClipboardToFocusedInput,
  resolveClipboardReader,
  resolveClipboardWriter,
  type ClipboardTextReader,
} from "./clipboard.js";
import {
  resolveVirtualKeyboard,
  shouldShowSoftKeyboard,
  type SoftKeyboardRequest,
} from "./soft-keyboard.js";

/** Native editable buffer bridge for GUI text inputs.
 *
 * Platform adapter for the optional `@ipp/react/gui` entry point: while
 * the canvas relay owns pointer, wheel and keyboard routing, real
 * editable text needs a natively focusable buffer for the OS IME,
 * soft keyboard and clipboard to target. This module mounts one hidden
 * `textarea` beside the canvas, keeps it focused only from trusted
 * activation gestures, and forwards every edit through the ordered sink
 * as ordinary `text` / composition / selection commands. Core keeps sole
 * text authority at all times: the buffer value is synced back from
 * committed core text, local edits `preventDefault` and forward instead
 * of applying, and every platform failure sends nothing.
 *
 * Focus fencing: an optional focus-token provider identifies the intended
 * target. Paste and user selection reads capture the token and re-check
 * it before sending, so delayed completions across a focus move, node
 * replacement or session change cancel explicitly instead of writing
 * into the new target.
 *
 * Caret units: core offsets index UTF-8 bytes while the DOM selects in
 * UTF-16 units; the conversion helpers below translate both ways snapped
 * to code-point boundaries. Candidate placement is caller-driven through
 * {@link TextBridgeHandle.placeAt} (last interaction point by default);
 * committed caret geometry arrives through the sync reader.
 *
 * Headless use imports no DOM dependencies: platform objects resolve
 * inside `attachTextBridge` only; every helper below is pure.
 */

/** Committed core text driving one buffer sync. Offsets index UTF-8 bytes. */
export interface TextBridgeCommitted {
  readonly text: string;
  readonly caretUtf8: number;
  readonly anchorUtf8: number | null;
  readonly composing?: boolean;
}

export interface TextBridgeOptions {
  readonly onError?: (error: Error) => void;
  /** Stable focus identity; see the module fencing note. */
  readonly getFocusToken?: () => unknown;
  /** Committed core text for the focused input, or null when unfocused. */
  readonly readCommitted?: () => TextBridgeCommitted | null;
  /** Observe user-driven buffer selections in core UTF-8 bytes; feeds fencing state. */
  readonly onLocalSelection?: (selection: TextBridgeLocalSelection) => void;
  /** `inputmode` for soft keyboards; defaults to `"text"`. */
  readonly inputMode?: string;
}

export interface TextBridgeHandle {
  /** The mounted buffer element. */
  readonly element: HTMLTextAreaElement;
  /** Move the buffer (IME candidate anchor) to container CSS pixels. */
  placeAt(x: number, y: number): void;
  /** Sync the buffer from committed core text; false without a reader. */
  syncFromCore(): boolean;
  /** Focus the buffer and request the soft keyboard on trusted gestures. */
  focusFromGesture(request: SoftKeyboardRequest): void;
  /** Focus-fenced clipboard paste into the core-focused input. */
  paste(reader: ClipboardTextReader | undefined): Promise<boolean>;
  /** Cancel composition, blur and remove the buffer. */
  dispose(): void;
}

/** Selected authoritative committed text, or null when copy/cut is invalid. */
export function selectedCommittedText(
  committed: TextBridgeCommitted | null,
): string | null {
  if (committed === null || committed.composing) return null;
  const anchor = committed.anchorUtf8 ?? committed.caretUtf8;
  const start = Math.min(anchor, committed.caretUtf8);
  const end = Math.max(anchor, committed.caretUtf8);
  if (start === end) return null;
  return committed.text.slice(
    utf8BytesToUtf16Units(committed.text, start),
    utf8BytesToUtf16Units(committed.text, end),
  );
}

/** UTF-16 code units to UTF-8 bytes, snapped to code-point boundaries. */
export function utf16UnitsToUtf8Bytes(text: string, units: number): number {
  const target = Math.max(0, Math.min(Math.floor(units), text.length));
  let seenUnits = 0;
  let bytes = 0;
  for (const point of text) {
    if (seenUnits + point.length > target) break;
    seenUnits += point.length;
    bytes += utf8ByteLength(point);
    if (seenUnits >= target) break;
  }
  return bytes;
}

/** UTF-8 bytes to UTF-16 code units, snapped to code-point boundaries. */
export function utf8BytesToUtf16Units(text: string, bytes: number): number {
  const target = Math.max(0, Math.min(Math.floor(bytes), utf8ByteLength(text)));
  let seenBytes = 0;
  let units = 0;
  for (const point of text) {
    const pointBytes = utf8ByteLength(point);
    if (seenBytes + pointBytes > target) break;
    seenBytes += pointBytes;
    units += point.length;
    if (seenBytes >= target) break;
  }
  return units;
}

/** Snap a UTF-8 byte offset to code-point boundaries, clamped to the text. */
export function clampUtf8Offset(text: string, offset: number): number {
  if (!Number.isFinite(offset)) return 0;
  const total = utf8ByteLength(text);
  const target = Math.floor(offset);
  if (target <= 0) return 0;
  if (target >= total) return total;
  let seen = 0;
  for (const point of text) {
    const next = seen + utf8ByteLength(point);
    if (next > target) break;
    seen = next;
  }
  return seen;
}

/** Container CSS pixels for the bridge anchor from a normalized viewport point. */
export interface BridgeContainerPoint {
  readonly x: number;
  readonly y: number;
}

/** Map a normalized canvas viewport point to container CSS pixels.
 *
 * Pure placement math for {@link TextBridgeHandle.placeAt}: the canvas rect
 * and its container rect come from `getBoundingClientRect`, so device-pixel
 * ratio cancels and out-of-canvas points extend linearly.
 */
export function viewportToBridgeOffset(
  viewport: GuiViewportPoint,
  canvasRect: {
    readonly left: number;
    readonly top: number;
    readonly width: number;
    readonly height: number;
  },
  containerRect: { readonly left: number; readonly top: number },
): BridgeContainerPoint {
  return {
    x: canvasRect.left - containerRect.left + viewport[0] * canvasRect.width,
    y: canvasRect.top - containerRect.top + viewport[1] * canvasRect.height,
  };
}

/** Focus identity plus committed revision fencing one text-bridge sync. */
export interface TextBridgeTarget {
  readonly session: bigint;
  readonly contextGeneration: bigint;
  readonly focusGeneration: bigint;
  readonly entity: bigint;
  readonly rootIncarnation: bigint;
  readonly node: number;
  readonly lifetime: number;
  readonly revision: number;
}

/** User-driven buffer selection in core UTF-8 byte offsets. `start` is the
 * anchor and `end` is the caret, so backward selections retain direction. */
export interface TextBridgeLocalSelection {
  readonly start: number;
  readonly end: number;
}

/** Focus/revision-fenced view of runtime text state for one bridge.
 *
 * Pure store with no DOM: the canvas flow folds the authoritative transient
 * focus record from runtime observations and reads the synchronous
 * `token`/`committed` accessors back out for the bridge options. Core stays
 * authoritative; no focus, text, selection or composition state is inferred
 * from control commits.
 *
 * The token includes session, input-context/focus generations, complete
 * target identity and the text revision. A local activation or selection
 * also advances an adapter fence so delayed clipboard reads cannot land
 * after a newer user intent while the runtime observation is in flight.
 */
export interface TextBridgeModel {
  /** Stable focus identity for `getFocusToken`; varies on every change. */
  token(): string;
  /** Committed text plus caret for `readCommitted`; null while unfocused. */
  committed(): TextBridgeCommitted | null;
  /** Fold one authoritative runtime focus update; undefined means no update. */
  observe(state: GuiTextFocusState | null | undefined): boolean;
  /** One local activation (trusted tap, Tab/Escape); invalidates in-flight reads. */
  noteActivation(): void;
  /** Track a user-driven buffer selection (core UTF-8 bytes); never syncs. */
  noteLocalSelection(start: number, end: number): void;
  /** Drop focus and text on blur, detach or session teardown. */
  clear(): void;
}

/** Create the fenced runtime-text view for one client session. */
export function createTextBridgeModel(session: bigint): TextBridgeModel {
  let focus: GuiTextFocusState | null = null;
  let localFence = 0;
  let needsResync = false;
  const sameState = (
    left: GuiTextFocusState,
    right: GuiTextFocusState,
  ): boolean =>
    left.session === right.session &&
    left.contextGeneration === right.contextGeneration &&
    left.focusGeneration === right.focusGeneration &&
    left.entity === right.entity &&
    left.rootIncarnation === right.rootIncarnation &&
    left.node === right.node &&
    left.lifetime === right.lifetime &&
    left.revision === right.revision &&
    left.text === right.text &&
    left.selectionStart === right.selectionStart &&
    left.selectionEnd === right.selectionEnd &&
    left.composition?.text === right.composition?.text &&
    left.composition?.caretStart === right.composition?.caretStart &&
    left.composition?.caretEnd === right.composition?.caretEnd;
  return {
    token(): string {
      const head = `s${session.toString(10)}:l${localFence}`;
      const current = focus;
      if (current === null) return head;
      return `${head}:c${current.contextGeneration}:f${current.focusGeneration}:n${current.entity}:${current.rootIncarnation}:${current.node}:${current.lifetime}:r${current.revision}`;
    },
    committed(): TextBridgeCommitted | null {
      const current = focus;
      if (current === null) return null;
      const composition = current.composition;
      if (composition !== undefined) {
        const start = Math.min(current.selectionStart, current.selectionEnd);
        const end = Math.max(current.selectionStart, current.selectionEnd);
        const startUnits = utf8BytesToUtf16Units(current.text, start);
        const endUnits = utf8BytesToUtf16Units(current.text, end);
        const text = `${current.text.slice(0, startUnits)}${composition.text}${current.text.slice(endUnits)}`;
        return {
          text,
          caretUtf8:
            start + clampUtf8Offset(composition.text, composition.caretEnd),
          anchorUtf8:
            start + clampUtf8Offset(composition.text, composition.caretStart),
          composing: true,
        };
      }
      return {
        text: current.text,
        caretUtf8: clampUtf8Offset(current.text, current.selectionEnd),
        anchorUtf8: clampUtf8Offset(current.text, current.selectionStart),
      };
    },
    observe(state): boolean {
      if (state === undefined) return false;
      if (state !== null && state.session !== session) return false;
      if (state === null) {
        localFence += 1;
        needsResync = false;
        if (focus === null) return false;
        focus = null;
        return true;
      }
      if (focus !== null && sameState(focus, state)) {
        if (!needsResync) return false;
        needsResync = false;
        return true;
      }
      focus = state;
      needsResync = false;
      return true;
    },
    noteActivation(): void {
      localFence += 1;
      needsResync = true;
    },
    noteLocalSelection(start, end): void {
      if (focus === null) return;
      localFence += 1;
      needsResync = true;
      focus = {
        ...focus,
        selectionStart: clampUtf8Offset(focus.text, start),
        selectionEnd: clampUtf8Offset(focus.text, end),
      };
    },
    clear(): void {
      focus = null;
      localFence += 1;
      needsResync = false;
    },
  };
}

/** Fold one runtime observation batch into the bridge model.
 *
 * Only the authoritative text-focus field changes the model. Ordinary
 * effects, conflicts, cancellations and unhandled input never imply focus.
 * Returns true when `syncFromCore` should run.
 */
export function observeTextBridgeBatch(
  model: TextBridgeModel,
  batch: GuiObservationBatch,
): boolean {
  return model.observe(batch.textFocus);
}

/** Mount the native editable buffer into a container beside the canvas.
 *
 * The buffer is visually hidden but focusable; it never takes focus
 * except through {@link TextBridgeHandle.focusFromGesture} on trusted
 * activation. Returns a handle; `dispose` cancels composition, blurs and
 * removes the element. The shared sink is never closed here: the canvas
 * relay owns sink revocation on detach.
 */
export function attachTextBridge(
  container: HTMLElement,
  sink: GuiInputSink,
  options: TextBridgeOptions = {},
): TextBridgeHandle {
  const {
    onError,
    getFocusToken,
    readCommitted,
    onLocalSelection,
    inputMode = "text",
  } = options;
  const report = (error: unknown): void => {
    onError?.(error instanceof Error ? error : new Error(String(error)));
  };
  const send = (command: Parameters<GuiInputSink["send"]>[0]): void => {
    try {
      sink.send(command);
    } catch (error) {
      report(error);
    }
  };

  const wrapper = document.createElement("div");
  wrapper.style.position = "absolute";
  wrapper.style.left = "0px";
  wrapper.style.top = "0px";
  wrapper.style.width = "0px";
  wrapper.style.height = "0px";
  wrapper.style.overflow = "hidden";
  const area = document.createElement("textarea");
  area.rows = 1;
  area.autocomplete = "off";
  area.setAttribute("autocorrect", "off");
  area.setAttribute("autocapitalize", "off");
  area.spellcheck = false;
  area.setAttribute("aria-label", "Text input buffer");
  area.inputMode = inputMode;
  area.style.position = "absolute";
  area.style.left = "0px";
  area.style.top = "0px";
  area.style.width = "2px";
  area.style.opacity = "0";
  area.style.pointerEvents = "none";
  wrapper.append(area);
  container.append(wrapper);

  const ime: ImeBridge = createImeBridge(sink, {
    ...(onError === undefined ? {} : { onError }),
  });
  // Suppress selection echoes of programmatic syncs; only user-driven
  // selection changes forward while the sync token still holds.
  let suppressSelection = false;
  let tokenAtSync: unknown = getFocusToken?.() ?? null;
  let disposed = false;

  const withSuppressedSelection = (apply: () => void): void => {
    suppressSelection = true;
    try {
      apply();
    } finally {
      suppressSelection = false;
    }
  };

  const onBeforeInput = (event: InputEvent): void => {
    if (disposed) return;
    if (shouldSkipBeforeInput(event.inputType)) return;
    event.preventDefault();
    // Clipboard events own paste/cut so their synchronous DataTransfer is
    // consumed exactly once. The paired beforeinput carries no extra edit.
    if (
      event.inputType === "insertFromPaste" ||
      event.inputType === "deleteByCut"
    ) {
      return;
    }
    if (typeof event.data === "string" && event.data.length > 0) {
      send({ kind: "text", text: event.data });
      return;
    }
    if (event.inputType === "deleteContentBackward") {
      send({ kind: "key", key: "backspace", pressed: true });
    } else if (event.inputType === "deleteContentForward") {
      send({ kind: "key", key: "delete", pressed: true });
    }
  };

  const onKeyDown = (event: KeyboardEvent): void => {
    if (disposed) return;
    const key = keyboardKeyToGuiKey(event.key);
    if (key === null) return;
    // Space, backspace and delete ride through beforeinput on an editable
    // buffer; forwarding them here as well would apply every keystroke
    // twice. All other mapped keys have no beforeinput payload.
    if (key === "space" || key === "backspace" || key === "delete") return;
    if (key === "tab") event.preventDefault();
    send({ kind: "key", key, pressed: true });
  };

  const onCompositionStart = (): void => {
    if (!disposed) ime.compositionStart();
  };
  const onCompositionUpdate = (event: Event): void => {
    if (!disposed) ime.compositionUpdate((event as CompositionEvent).data);
  };
  const onCompositionEnd = (event: Event): void => {
    if (!disposed) ime.compositionEnd((event as CompositionEvent).data);
  };

  // Last forwarded/applied selection in core UTF-8 bytes; identical echoes
  // of document selectionchange plus element select send once.
  let lastSentSelection: TextBridgeLocalSelection | null = null;

  const onSelect = (): void => {
    if (disposed || suppressSelection || ime.isComposing()) return;
    if (document.activeElement !== null && document.activeElement !== area) {
      return;
    }
    if (getFocusToken !== undefined && getFocusToken() !== tokenAtSync) return;
    const { selectionStart, selectionEnd, selectionDirection, value } = area;
    if (selectionStart === null || selectionEnd === null) return;
    const backward = selectionDirection === "backward";
    const start = utf16UnitsToUtf8Bytes(
      value,
      backward ? selectionEnd : selectionStart,
    );
    const end = utf16UnitsToUtf8Bytes(
      value,
      backward ? selectionStart : selectionEnd,
    );
    if (
      lastSentSelection !== null &&
      lastSentSelection.start === start &&
      lastSentSelection.end === end
    ) {
      return;
    }
    lastSentSelection = { start, end };
    onLocalSelection?.({ start, end });
    // An accepted local selection advances the model's clipboard/focus fence,
    // but the DOM still represents that same intent. Follow the new token so
    // another user selection can supersede it before the runtime echo arrives.
    tokenAtSync = getFocusToken?.() ?? tokenAtSync;
    send({ kind: "selection", start, end });
  };

  const onBlur = (): void => {
    ime.blur();
  };

  const onPaste = (event: ClipboardEvent): void => {
    if (disposed || readCommitted?.() === null) return;
    event.preventDefault();
    const transfer = event.clipboardData;
    if (transfer !== null) {
      const text = transfer.getData("text/plain");
      if (text.length === 0) return;
      if (utf8ByteLength(text) > CLIPBOARD_TEXT_MAX_BYTES) {
        report(new Error("Clipboard text exceeds the ingress bound"));
        return;
      }
      send({ kind: "text", text });
      return;
    }
    void pasteClipboardToFocusedInput(sink, resolveClipboardReader(), {
      ...(getFocusToken === undefined ? {} : { getFocusToken }),
      ...(onError === undefined ? {} : { onError }),
    });
  };

  const copyOrCut = (event: ClipboardEvent, cut: boolean): void => {
    if (disposed) return;
    const text = selectedCommittedText(readCommitted?.() ?? null);
    if (text === null) return;
    event.preventDefault();
    if (event.clipboardData !== null) {
      event.clipboardData.setData("text/plain", text);
      if (cut) send({ kind: "key", key: "delete", pressed: true });
      return;
    }
    const token = getFocusToken?.();
    void copyFocusedTextToClipboard(text, resolveClipboardWriter(), {
      ...(onError === undefined ? {} : { onError }),
    }).then((copied) => {
      if (!copied || !cut) return;
      if (getFocusToken !== undefined && getFocusToken() !== token) {
        report(
          new Error("Clipboard cut cancelled: focus changed while writing"),
        );
        return;
      }
      send({ kind: "key", key: "delete", pressed: true });
    });
  };

  const onCopy = (event: ClipboardEvent): void => {
    copyOrCut(event, false);
  };

  const onCut = (event: ClipboardEvent): void => {
    copyOrCut(event, true);
  };

  area.addEventListener("beforeinput", onBeforeInput as EventListener);
  area.addEventListener("keydown", onKeyDown as EventListener);
  area.addEventListener("compositionstart", onCompositionStart);
  area.addEventListener("compositionupdate", onCompositionUpdate);
  area.addEventListener("compositionend", onCompositionEnd);
  area.addEventListener("select", onSelect);
  area.addEventListener("blur", onBlur);
  area.addEventListener("paste", onPaste);
  area.addEventListener("copy", onCopy);
  area.addEventListener("cut", onCut);
  // Caret moves that never change the value (arrow keys, pointer taps)
  // surface here rather than on the element alone.
  document.addEventListener("selectionchange", onSelect);

  return {
    element: area,
    placeAt(x: number, y: number): void {
      if (disposed) return;
      wrapper.style.transform = `translate(${x}px, ${y}px)`;
    },
    syncFromCore(): boolean {
      if (disposed || readCommitted === undefined) return false;
      const committed = readCommitted();
      tokenAtSync = getFocusToken?.() ?? null;
      if (committed === null) {
        if (document.activeElement === area) area.blur();
        lastSentSelection = null;
        withSuppressedSelection(() => {
          area.value = "";
        });
        return true;
      }
      const anchorBytes = committed.anchorUtf8 ?? committed.caretUtf8;
      lastSentSelection = {
        start: anchorBytes,
        end: committed.caretUtf8,
      };
      withSuppressedSelection(() => {
        if (
          committed.composing &&
          ime.isComposing() &&
          area.value === committed.text
        ) {
          return;
        }
        if (area.value !== committed.text) area.value = committed.text;
        const caret = utf8BytesToUtf16Units(
          committed.text,
          committed.caretUtf8,
        );
        const anchor =
          committed.anchorUtf8 === null
            ? caret
            : utf8BytesToUtf16Units(committed.text, committed.anchorUtf8);
        area.setSelectionRange(
          Math.min(anchor, caret),
          Math.max(anchor, caret),
          anchor > caret ? "backward" : anchor < caret ? "forward" : "none",
        );
      });
      return true;
    },
    focusFromGesture(request: SoftKeyboardRequest): void {
      if (disposed || !shouldShowSoftKeyboard(request)) return;
      area.focus({ preventScroll: true });
      const keyboard = resolveVirtualKeyboard();
      if (keyboard === undefined) return;
      try {
        const shown = keyboard.show();
        if (shown instanceof Promise) {
          shown.catch(report);
        }
      } catch (error) {
        report(error);
      }
    },
    paste(reader: ClipboardTextReader | undefined): Promise<boolean> {
      if (disposed) return Promise.resolve(false);
      return pasteClipboardToFocusedInput(sink, reader, {
        ...(getFocusToken === undefined ? {} : { getFocusToken }),
        ...(onError === undefined ? {} : { onError }),
      });
    },
    dispose(): void {
      if (disposed) return;
      disposed = true;
      ime.blur();
      area.removeEventListener("beforeinput", onBeforeInput as EventListener);
      area.removeEventListener("keydown", onKeyDown as EventListener);
      area.removeEventListener("compositionstart", onCompositionStart);
      area.removeEventListener("compositionupdate", onCompositionUpdate);
      area.removeEventListener("compositionend", onCompositionEnd);
      area.removeEventListener("select", onSelect);
      area.removeEventListener("blur", onBlur);
      area.removeEventListener("paste", onPaste);
      area.removeEventListener("copy", onCopy);
      area.removeEventListener("cut", onCut);
      document.removeEventListener("selectionchange", onSelect);
      wrapper.remove();
    },
  };
}
