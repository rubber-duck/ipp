import { useCallback, useLayoutEffect, useRef, useState } from "react";
import type { GuiClientInput, GuiEffectSubscription } from "@ipp/client";
import { useWorld } from "../scope/world.js";
import type { GuiControlHandle } from "./control-ref.js";

/** Lease the generated client's input and focus stream for the exact mounted control.
 * Fence synchronous replacement as well as asynchronous subscribe/unsubscribe replies. */
export function useTargetInput(
  onInput: (input: GuiClientInput) => void,
  onBlur?: () => void,
  onFocus?: () => void,
) {
  const { client } = useWorld();
  const current = useRef<GuiControlHandle | null>(null);
  const callbacks = useRef({ onInput, onBlur, onFocus });
  const [handle, setHandle] = useState<GuiControlHandle | null>(null);
  const [failure, setFailure] = useState<{
    handle: GuiControlHandle;
    error: unknown;
  } | null>(null);
  useLayoutEffect(() => {
    callbacks.current = { onInput, onBlur, onFocus };
  }, [onInput, onBlur, onFocus]);
  const attach = useCallback((next: GuiControlHandle | null) => {
    if (current.current === next) return;
    if (current.current) callbacks.current.onBlur?.();
    current.current = next;
    setHandle(next);
  }, []);
  useLayoutEffect(() => {
    if (!handle) return;
    if (!client.subscribeGuiEffects)
      throw new Error("Input targets require a generated GUI client");
    let live = true;
    let subscription: GuiEffectSubscription | undefined;
    const active = () => live && current.current === handle;
    const report = (error: unknown) => {
      if (active()) setFailure({ handle, error });
    };
    void client
      .subscribeGuiEffects(
        (event) => {
          if (!active()) return;
          const effect = event.effect;
          if (effect.kind === "input") callbacks.current.onInput(effect.input);
          else if (effect.kind === "focusChanged") {
            if (effect.focused) callbacks.current.onFocus?.();
            else callbacks.current.onBlur?.();
          }
        },
        { inputTarget: handle.target },
      )
      .then(async (received) => {
        subscription = received;
        if (!active()) {
          await received.unsubscribe();
          return;
        }
        const result = await received.closed;
        if (result.kind === "closed") report(result.reason);
      })
      .catch(report);
    return () => {
      live = false;
      void subscription?.unsubscribe().catch(() => {});
    };
  }, [client, handle]);
  if (failure && failure.handle === handle) throw failure.error;
  return attach;
}
