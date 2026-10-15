import { useRef } from "react";
import { useComposedControlRef } from "../../gui/control-refs.js";
import type { GuiControlEvent } from "../../gui/callbacks.js";
import type { GuiControlHandle, GuiControlRef } from "../../gui/control-ref.js";
import { useValueSynchronization } from "./value-synchronization.js";

/** Actual native reports remain distinct from the application's controlled intent. */
export function useCheckedValue(
  checked: boolean | undefined,
  defaultChecked: boolean,
  onCheckedChange: ((value: boolean) => void) | undefined,
  ref: GuiControlRef | undefined,
) {
  const handle = useRef<GuiControlHandle | null>(null);
  const state = useValueSynchronization({
    value: checked,
    defaultValue: defaultChecked,
    onChange: onCheckedChange,
    same: Object.is,
    identity: () => handle.current,
    write: async (from, to, context) => {
      const target = handle.current;
      if (target)
        await context.trackEcho(to, () =>
          target.compareAndSet("checked", from, to),
        );
    },
  });
  const attach = useComposedControlRef(handle, ref, state.notifyReady);
  return {
    current: state.current,
    initial: state.initial,
    changed: (event: GuiControlEvent<boolean>) =>
      state.observe(event.value, event),
    ref: attach,
  };
}
