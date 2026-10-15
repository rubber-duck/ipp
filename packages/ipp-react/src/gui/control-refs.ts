import { useMemo, type RefObject } from "react";
import type { GuiControlHandle, GuiControlRef } from "./control-ref.js";

/** Compose refs at commit time, retaining each callback's cleanup and identity. */
export function useComposedControlRef(
  own: RefObject<GuiControlHandle | null>,
  ref: GuiControlRef | undefined,
  attached?: (handle: GuiControlHandle) => void,
): GuiControlRef {
  return useMemo(() => {
    let active: (() => void) | undefined;
    return (handle: GuiControlHandle | null) => {
      active?.();
      if (!handle) return;
      let cleanup: void | (() => void);
      let assigning = true;
      let pending = false;
      let released = false;
      const release = () => {
        if (released) return;
        if (assigning) {
          pending = true;
          return;
        }
        released = true;
        if (active === release) active = undefined;
        if (own.current === handle) own.current = null;
        if (typeof cleanup === "function") cleanup();
        else if (typeof ref === "function") ref(null);
        else if (ref?.current === handle) ref.current = null;
      };
      active = release;
      own.current = handle;
      try {
        if (typeof ref === "function") cleanup = ref(handle);
        else if (ref) ref.current = handle;
        assigning = false;
        if (pending) release();
        else attached?.(handle);
      } catch (error) {
        assigning = false;
        try {
          release();
        } catch (cleanupError) {
          throw new AggregateError(
            [error, cleanupError],
            "Control ref assignment and cleanup failed",
          );
        }
        throw error;
      }
      return release;
    };
  }, [own, ref, attached]);
}
