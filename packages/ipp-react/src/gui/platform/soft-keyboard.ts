/** Soft-keyboard trusted-gesture policy hooks for GUI text inputs.
 *
 * Platform adapter slice for the optional `@ipp/react/gui` entry point:
 * on touch devices the OS soft keyboard must be requested from a trusted
 * user gesture (tap / pointer activation on a focused text input), never
 * from background or programmatic code paths. These hooks decide whether
 * a keyboard show request is allowed; the platform (`navigator.
 * virtualKeyboard`, hidden-input focus, `inputMode`) still owns the
 * keyboard itself, and core keeps text authority throughout — a denied or
 * failed request sends no input command and changes no focus.
 *
 * The policy follows the runtime focus fence: showing the keyboard is tied
 * to genuine activation gestures, and blur always allows dismissal.
 * Headless use imports no DOM dependencies: platform objects resolve
 * inside the functions via `globalThis`.
 */

/** Activation gesture kinds that may request the soft keyboard. */
export type SoftKeyboardTrigger =
  | "pointerDown"
  | "tap"
  | "key"
  | "programmatic";

/** One soft-keyboard show request with its gesture provenance. */
export interface SoftKeyboardRequest {
  /** Which activation produced the request. */
  readonly trigger: SoftKeyboardTrigger;
  /** DOM `event.isTrusted`: false for synthesized events. */
  readonly isTrusted: boolean;
}

export interface SoftKeyboardPolicyOptions {
  /** Also allow trusted key activation to show the keyboard. */
  readonly allowKeyTrigger?: boolean;
}

/** Structural virtual-keyboard subset (`navigator.virtualKeyboard`). */
export interface VirtualKeyboardLike {
  show(): Promise<void> | void;
  hide(): Promise<void> | void;
}

/** Whether the gesture kind alone can ever request the keyboard. */
export function isTrustedSoftKeyboardTrigger(
  trigger: SoftKeyboardTrigger,
): boolean {
  return trigger === "pointerDown" || trigger === "tap";
}

/** Decide whether a show request may reach the platform keyboard.
 *
 * Allowed only for trusted pointer/tap activation (and, opt-in, trusted
 * key activation). Programmatic requests and untrusted (synthesized)
 * events are always denied: they cannot prove a user gesture, so the
 * keyboard stays down and no input command is sent.
 */
export function shouldShowSoftKeyboard(
  request: SoftKeyboardRequest,
  options: SoftKeyboardPolicyOptions = {},
): boolean {
  if (!request.isTrusted) return false;
  if (isTrustedSoftKeyboardTrigger(request.trigger)) return true;
  return request.trigger === "key" && (options.allowKeyTrigger ?? false);
}

/** Resolve the platform virtual keyboard without throwing. */
export function resolveVirtualKeyboard(
  navigatorLike: unknown = globalNavigator(),
): VirtualKeyboardLike | undefined {
  if (typeof navigatorLike !== "object" || navigatorLike === null) {
    return undefined;
  }
  const keyboard = (navigatorLike as { virtualKeyboard?: unknown })
    .virtualKeyboard;
  if (typeof keyboard !== "object" || keyboard === null) return undefined;
  const show = (keyboard as Record<string, unknown>)["show"];
  const hide = (keyboard as Record<string, unknown>)["hide"];
  if (typeof show !== "function" || typeof hide !== "function") {
    return undefined;
  }
  return keyboard as VirtualKeyboardLike;
}

function globalNavigator(): unknown {
  return (globalThis as { navigator?: unknown }).navigator;
}
