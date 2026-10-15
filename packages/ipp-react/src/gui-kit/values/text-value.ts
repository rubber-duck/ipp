import { useRef } from "react";
import { useComposedControlRef } from "../../gui/control-refs.js";
import type { GuiControlEvent } from "../../gui/callbacks.js";
import type { GuiControlHandle, GuiControlRef } from "../../gui/control-ref.js";
import { useValueSynchronization } from "./value-synchronization.js";

/** Actual native reports remain distinct from the application's controlled intent. */
export function useTextValue(
  value: string | undefined,
  defaultValue: string,
  onChange: ((value: string) => void) | undefined,
  ref: GuiControlRef | undefined,
) {
  const handle = useRef<GuiControlHandle | null>(null);
  const state = useValueSynchronization({
    value: value,
    defaultValue: defaultValue,
    onChange: onChange,
    same: Object.is,
    identity: () => handle.current,
    write: async (from, to, context) => {
      const target = handle.current;
      if (target)
        await context.trackEcho(to, () =>
          target.compareAndSet("text", from, to),
        );
    },
  });
  const attach = useComposedControlRef(handle, ref, state.notifyReady);
  return {
    current: state.current,
    initial: state.initial,
    changed: (event: GuiControlEvent<string>) =>
      state.observe(event.value, event),
    handle,
    ref: attach,
  };
}
