import type { OverlayOpen } from "./overlay.js";

/** Application activation policy composed after a native control press. */
export interface PopupActivationOptions<Value> {
  /** Shared open intent; omit for a menu outside an overlay. */
  readonly overlay?: OverlayOpen;
  /** Close before notifying the application; false for a toggling option list. */
  readonly closeOnActivate?: boolean;
  readonly onActivate?: (value: Value) => void;
}

/**
 * Compose a native press with application activation. Read the overlay's live
 * intent for every delivery, so a second queued press after closing is ignored.
 * Closing precedes the application callback, including when it rerenders or
 * reenters synchronously. A controlled owner may subsequently reject the close
 * through the existing acknowledged visibility synchronization.
 *
 * This keeps no selection, focus, active-item or target state. Native GUI owns
 * eligibility and input; the reconciler fences callbacks by session and exact
 * control lifetime. Callers compose selection/filtering/text writes in onActivate
 * and rebuild this callback with each committed render.
 */
export function popupActivation<Value>({
  overlay,
  closeOnActivate = true,
  onActivate,
}: PopupActivationOptions<Value>): (value: Value) => void {
  return (value) => {
    if (overlay && !overlay.open) return;
    if (closeOnActivate) overlay?.setOpen(false);
    onActivate?.(value);
  };
}
