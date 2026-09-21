import type { BrowserGuiInputCommand, GuiInputSink } from "./input.js";

/** IME composition bridge for GUI text inputs (W3C Input Events).
 *
 * Platform adapter slice for the optional `@ipp/react/gui` entry point:
 * DOM `compositionstart` / `compositionupdate` / `compositionend` and
 * `beforeinput` payloads translate into the ordered composition commands
 * owned by core (`.8`: `UpdateComposition` / `CommitComposition` /
 * `CancelComposition`) through the existing ordered ingress sink (`.11`:
 * `GuiInputSink` into the correlated `RequestBody::GuiInput` path, or
 * `WorldContext::enqueue_gui_input_command` for host-owned loops).
 *
 * Ownership: core `GuiInputSystem` owns the provisional composition buffer,
 * revision and focus fencing, and conflict decisions. This module retains
 * only the open flag and last provisional payload needed to make a terminal
 * `compositionend.data` authoritative when a platform omits its matching
 * final update. It never mutates scene state or re-decides routing. Headless
 * use imports no DOM dependencies: the DOM is touched only inside
 * {@link attachImeBridge}.
 *
 * W3C sequence per composition session: `compositionstart`, then one or
 * more `compositionupdate` provisional payloads (each paired with a
 * `beforeinput` of type `insertCompositionText`), then `compositionend`
 * carrying the committed string on accept or empty data on cancel,
 * followed by a `beforeinput` of type `insertFromComposition` on accept.
 * Every provisional therefore applies exactly once: updates flow through
 * `compositionupdate` only, and both `beforeinput` composition payloads
 * are skipped (see {@link shouldSkipBeforeInput}). `keydown` 229 handling
 * stays absent deliberately: `beforeinput` remains the text source of
 * truth, as in the `.11` relay.
 *
 * Caret offsets index UTF-8 bytes (`.8`); browsers report no caret, so
 * each provisional carries a caret collapsed at its UTF-8 end.
 */

export interface ImeBridgeOptions {
  readonly onError?: (error: Error) => void;
}

/** Minimal event target for IME listeners; real elements satisfy this. */
export interface ImeEventTarget {
  addEventListener(type: string, listener: EventListener): void;
  removeEventListener(type: string, listener: EventListener): void;
}

/** UTF-8 byte length of a string (caret unit for composition commands). */
export function utf8ByteLength(text: string): number {
  return new TextEncoder().encode(text).length;
}

/** Map a `compositionupdate` payload to a provisional update command.
 *
 * Returns `null` for empty or missing data (cancel paths and duplicate
 * ends carry no provisional), so callers send nothing and keep no stale
 * text. Nonempty data maps to a `composition` command with the caret
 * collapsed at its UTF-8 end.
 */
export function mapCompositionUpdate(
  data: string | null | undefined,
): BrowserGuiInputCommand | null {
  if (typeof data !== "string" || data.length === 0) return null;
  const caret = utf8ByteLength(data);
  return {
    kind: "composition",
    text: data,
    caretStart: caret,
    caretEnd: caret,
  };
}

/** Map a `compositionend` payload to its terminal command.
 *
 * A nonempty end commits the provisional built by earlier updates; an
 * empty or missing end (cancelled composition) cancels it. Either way the
 * session closes and the core focus/revision fence decides admission.
 */
export function mapCompositionEnd(
  data: string | null | undefined,
): BrowserGuiInputCommand {
  if (typeof data === "string" && data.length > 0) {
    return { kind: "commitComposition" };
  }
  return { kind: "cancelComposition" };
}

/** Whether a `beforeinput` payload belongs to an IME session.
 *
 * Both W3C composition payloads are skipped: `insertCompositionText`
 * duplicates the paired `compositionupdate`, and `insertFromComposition`
 * duplicates the `compositionend` commit. Plain `insertText`, pastes and
 * deletions are never skipped here.
 */
export function shouldSkipBeforeInput(inputType: string): boolean {
  return (
    inputType === "insertCompositionText" ||
    inputType === "insertFromComposition" ||
    inputType.startsWith("insertComposition")
  );
}

/** Ordered IME session tracker forwarding through a GUI input sink. */
export interface ImeBridge {
  /** Whether a composition session is currently open. */
  isComposing(): boolean;
  /** Open a session; cancels a stale still-open session first. */
  compositionStart(): void;
  /** Forward one provisional update; ignores empty payloads. */
  compositionUpdate(data: string | null | undefined): void;
  /** Commit or cancel the session and close it. */
  compositionEnd(data: string | null | undefined): void;
  /** Cancel an open session on platform blur; idle blur sends nothing. */
  blur(): void;
}

/** Create an IME session tracker over an ordered GUI ingress sink.
 *
 * Sends serialize in call order through the sink. A `compositionstart`
 * arriving while a session is open cancels the stale provisional first
 * (no silent merge, mirroring `.8` direct-edit cancellation); blur
 * cancels an open session (mirroring the `.8` focus fence) and is a
 * no-op while idle.
 */
export function createImeBridge(
  sink: GuiInputSink,
  options: ImeBridgeOptions = {},
): ImeBridge {
  const { onError } = options;
  let composing = false;
  let lastProvisional: string | null = null;
  const send = (command: BrowserGuiInputCommand): void => {
    try {
      sink.send(command);
    } catch (error) {
      onError?.(error instanceof Error ? error : new Error(String(error)));
    }
  };
  return {
    isComposing: () => composing,
    compositionStart: () => {
      if (composing) send({ kind: "cancelComposition" });
      composing = true;
      lastProvisional = null;
    },
    compositionUpdate: (data) => {
      const command = mapCompositionUpdate(data);
      if (command === null) return;
      composing = true;
      lastProvisional = data as string;
      send(command);
    },
    compositionEnd: (data) => {
      composing = false;
      if (
        typeof data === "string" &&
        data.length > 0 &&
        data !== lastProvisional
      ) {
        const command = mapCompositionUpdate(data);
        if (command !== null) send(command);
      }
      send(mapCompositionEnd(data));
      lastProvisional = null;
    },
    blur: () => {
      if (!composing) return;
      composing = false;
      lastProvisional = null;
      send({ kind: "cancelComposition" });
    },
  };
}

export interface AttachImeBridgeOptions {
  readonly onError?: (error: Error) => void;
}

/** Attach IME composition listeners to a keyboard target.
 *
 * Forwards `compositionstart` / `compositionupdate` / `compositionend`
 * through a session tracker into the ordered sink. Use this for focused
 * text-input hosts (for example a hidden input element) that do not use
 * the combined canvas relay; `attachCanvasGuiInput` already wires the
 * same composition mapping onto its own keyboard target, so attaching
 * both to one element would double-send. Returns a detach function
 * removing every listener.
 */
export function attachImeBridge(
  target: ImeEventTarget,
  sink: GuiInputSink,
  options: AttachImeBridgeOptions = {},
): () => void {
  const bridge = createImeBridge(sink, options);
  const onStart = (): void => {
    bridge.compositionStart();
  };
  const onUpdate = (event: Event): void => {
    bridge.compositionUpdate((event as CompositionEvent).data);
  };
  const onEnd = (event: Event): void => {
    bridge.compositionEnd((event as CompositionEvent).data);
  };
  target.addEventListener("compositionstart", onStart);
  target.addEventListener("compositionupdate", onUpdate);
  target.addEventListener("compositionend", onEnd);
  return () => {
    target.removeEventListener("compositionstart", onStart);
    target.removeEventListener("compositionupdate", onUpdate);
    target.removeEventListener("compositionend", onEnd);
  };
}
