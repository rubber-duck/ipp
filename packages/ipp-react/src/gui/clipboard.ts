import type { GuiInputSink } from "./input.js";

/** Clipboard read/write bridge for focused GUI text inputs.
 *
 * Platform adapter slice for the optional `@ipp/react/gui` entry point:
 * the browser clipboard supplies paste text into the core-focused
 * `TextInput` (via the ordered `.11` sink as an ordinary `text` command)
 * and accepts committed text copied out of it. Core keeps sole text
 * authority at all times: platform failure — denied permission, insecure
 * context without `navigator.clipboard`, or a rejected read/write —
 * never transfers authority to the DOM and never implies a successful
 * edit. Every failure path sends no input command and reports through
 * `onError` instead.
 *
 * Focus: these helpers never move or claim focus. A paste sends an
 * unfocused `text` command that core routes to its focused `TextInput`;
 * with no eligible focus it is `unhandled` there (`.8`), which is the
 * correct outcome rather than focus stealing. A copy takes the committed
 * text the caller read from the semantic snapshot or inspect response for
 * the focused node; the DOM is never treated as the value source.
 *
 * Permissions are honored before touching the clipboard: an explicit
 * `denied` state short-circuits without calling `readText`/`writeText`
 * (no permission prompt is triggered by a denied check). When the
 * Permissions API is absent the clipboard call runs and its rejection is
 * handled the same way. Headless use imports no DOM dependencies:
 * platform objects resolve inside the functions via `globalThis`.
 */

/** Structural clipboard text reader (`navigator.clipboard` subset). */
export interface ClipboardTextReader {
  readText(): Promise<string>;
}

/** Structural clipboard text writer (`navigator.clipboard` subset). */
export interface ClipboardTextWriter {
  writeText(text: string): Promise<void>;
}

/** Clipboard permission state; `unknown` means unqueryable here. */
export type ClipboardPermissionState =
  | "granted"
  | "denied"
  | "prompt"
  | "unknown";

/** Wire string bound mirrored from the `.11` ingress (`.8` caps further). */
export const CLIPBOARD_TEXT_MAX_BYTES = 65536;

function globalNavigator(): unknown {
  return (globalThis as { navigator?: unknown }).navigator;
}

function pickMethod<T>(holder: unknown, name: string): T | undefined {
  if (typeof holder !== "object" || holder === null) return undefined;
  const method = (holder as Record<string, unknown>)[name];
  return typeof method === "function" ? (method as T) : undefined;
}

/** Resolve a clipboard text reader without throwing when unavailable. */
export function resolveClipboardReader(
  navigatorLike: unknown = globalNavigator(),
): ClipboardTextReader | undefined {
  if (typeof navigatorLike !== "object" || navigatorLike === null) {
    return undefined;
  }
  const clipboard = (navigatorLike as { clipboard?: unknown }).clipboard;
  const readText = pickMethod<ClipboardTextReader["readText"]>(
    clipboard,
    "readText",
  );
  if (readText === undefined) return undefined;
  return { readText: readText.bind(clipboard) };
}

/** Resolve a clipboard text writer without throwing when unavailable. */
export function resolveClipboardWriter(
  navigatorLike: unknown = globalNavigator(),
): ClipboardTextWriter | undefined {
  if (typeof navigatorLike !== "object" || navigatorLike === null) {
    return undefined;
  }
  const clipboard = (navigatorLike as { clipboard?: unknown }).clipboard;
  const writeText = pickMethod<ClipboardTextWriter["writeText"]>(
    clipboard,
    "writeText",
  );
  if (writeText === undefined) return undefined;
  return { writeText: writeText.bind(clipboard) };
}

/** Query a clipboard permission without throwing when unqueryable. */
export async function queryClipboardPermission(
  kind: "clipboard-read" | "clipboard-write",
  permissionsLike: unknown = globalPermissions(),
): Promise<ClipboardPermissionState> {
  const query = pickMethod<
    (request: { name: string }) => Promise<{ state: string }>
  >(permissionsLike, "query");
  if (query === undefined) return "unknown";
  let state: string;
  try {
    state = (await query.call(permissionsLike, { name: kind })).state;
  } catch {
    return "unknown";
  }
  if (state === "granted" || state === "denied" || state === "prompt") {
    return state;
  }
  return "unknown";
}

function globalPermissions(): unknown {
  const navigatorLike = globalNavigator();
  if (typeof navigatorLike !== "object" || navigatorLike === null) {
    return undefined;
  }
  return (navigatorLike as { permissions?: unknown }).permissions;
}

export interface ClipboardReadOptions {
  readonly permissions?: unknown;
  readonly maxBytes?: number | undefined;
}

export type ClipboardReadResult =
  | { readonly ok: true; readonly text: string }
  | { readonly ok: false; readonly reason: string };

/** Read clipboard text without ever implying an edit.
 *
 * Returns the text on success (possibly empty: the caller then sends
 * nothing, since empty `Text` is `Err(InvalidValue)` at dispatch in
 * `.8`). Every failure — unavailable clipboard, denied permission,
 * rejected read, non-string payload, oversize payload — returns
 * `{ ok: false }` with a reason and no text.
 */
export async function readClipboardText(
  reader: ClipboardTextReader | undefined,
  options: ClipboardReadOptions = {},
): Promise<ClipboardReadResult> {
  if (reader === undefined) {
    return { ok: false, reason: "Clipboard is unavailable in this context" };
  }
  const { permissions, maxBytes = CLIPBOARD_TEXT_MAX_BYTES } = options;
  const state = await queryClipboardPermission("clipboard-read", permissions);
  if (state === "denied") {
    return { ok: false, reason: "Clipboard read permission was denied" };
  }
  let text: unknown;
  try {
    text = await reader.readText();
  } catch (error) {
    const failure = error instanceof Error ? error : new Error(String(error));
    return { ok: false, reason: failure.message };
  }
  if (typeof text !== "string") {
    return { ok: false, reason: "Clipboard returned a non-string payload" };
  }
  if (text.length === 0) return { ok: true, text: "" };
  if (new TextEncoder().encode(text).length > maxBytes) {
    return { ok: false, reason: "Clipboard text exceeds the ingress bound" };
  }
  return { ok: true, text };
}

export interface ClipboardWriteOptions {
  readonly permissions?: unknown;
}

export type ClipboardWriteResult =
  | { readonly ok: true }
  | { readonly ok: false; readonly reason: string };

/** Write committed text to the clipboard without implying an edit.
 *
 * Copying never sends input commands: the text argument is already the
 * core-committed value, and a write failure reports `{ ok: false }`
 * without touching ingress. Empty text writes nothing and reports
 * `{ ok: false }` so callers never clear the clipboard by accident.
 */
export async function writeClipboardText(
  text: string,
  writer: ClipboardTextWriter | undefined,
  options: ClipboardWriteOptions = {},
): Promise<ClipboardWriteResult> {
  if (writer === undefined) {
    return { ok: false, reason: "Clipboard is unavailable in this context" };
  }
  if (text.length === 0) {
    return { ok: false, reason: "Nothing to copy" };
  }
  const state = await queryClipboardPermission(
    "clipboard-write",
    options.permissions,
  );
  if (state === "denied") {
    return { ok: false, reason: "Clipboard write permission was denied" };
  }
  try {
    await writer.writeText(text);
  } catch (error) {
    const failure = error instanceof Error ? error : new Error(String(error));
    return { ok: false, reason: failure.message };
  }
  return { ok: true };
}

export interface ClipboardSinkOptions {
  readonly permissions?: unknown;
  readonly maxBytes?: number | undefined;
  readonly onError?: (error: Error) => void;
  /**
   * Stable focus identity for the paste target (focused node id, handle
   * string or generation counter). Captured before the clipboard read and
   * re-checked after it resolves: a focus move, node replacement or
   * session change while the read waited cancels the paste explicitly
   * instead of writing into the new target. Omitted means unfenced.
   */
  readonly getFocusToken?: () => unknown;
}

/** Paste clipboard text into the core-focused text input.
 *
 * On success sends one ordered `{ kind: "text" }` command carrying the
 * clipboard string (insert at caret / replace selection happens in core
 * under the `.8` revision gate). Returns `true` only when a command was
 * sent. Empty clipboard text sends nothing (returns `false`); a focus
 * change across the read wait cancels explicitly without sending; any
 * other failure sends nothing, reports through `onError`, and returns
 * `false`: failure never transfers text authority.
 */
export async function pasteClipboardToFocusedInput(
  sink: GuiInputSink,
  reader: ClipboardTextReader | undefined,
  options: ClipboardSinkOptions = {},
): Promise<boolean> {
  const { permissions, maxBytes, onError, getFocusToken } = options;
  const token = getFocusToken?.();
  const result = await readClipboardText(reader, {
    ...(permissions === undefined ? {} : { permissions }),
    ...(maxBytes === undefined ? {} : { maxBytes }),
  });
  if (!result.ok) {
    onError?.(new Error(result.reason));
    return false;
  }
  if (result.text.length === 0) return false;
  if (getFocusToken !== undefined && getFocusToken() !== token) {
    onError?.(
      new Error("Clipboard paste cancelled: focus changed while reading"),
    );
    return false;
  }
  try {
    sink.send({ kind: "text", text: result.text });
  } catch (error) {
    onError?.(error instanceof Error ? error : new Error(String(error)));
    return false;
  }
  return true;
}

/** Copy focused committed text out to the clipboard.
 *
 * The `text` argument must be the core-committed value (semantic
 * snapshot / inspect response), never DOM content. Returns `true` on a
 * completed write; any failure reports through `onError` and returns
 * `false` without sending input commands.
 */
export async function copyFocusedTextToClipboard(
  text: string,
  writer: ClipboardTextWriter | undefined,
  options: ClipboardSinkOptions = {},
): Promise<boolean> {
  const { permissions, onError } = options;
  if (text.length === 0) return false;
  const result = await writeClipboardText(text, writer, {
    ...(permissions === undefined ? {} : { permissions }),
  });
  if (!result.ok) {
    onError?.(new Error(result.reason));
    return false;
  }
  return true;
}
