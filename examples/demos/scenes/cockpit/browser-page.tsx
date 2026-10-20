import { useEffect, useState } from "react";
import { Children, Entity, useWorld } from "@ipp/react";
import {
  Behavior,
  Html,
  InputTarget,
  Layout,
  type GuiControlHandle,
} from "@ipp/react/gui";
import type { GuiEffectSubscription, HtmlSource } from "@ipp/client";
import { TextLine } from "@ipp/react/gui-kit";

/** The same incarnation drives the HTML image and its exclusive input stream. */
export function BrowserPage({
  registration,
  status,
  width,
  height,
  onError,
}: {
  registration: HtmlSource | undefined;
  status: string;
  width: number;
  height: number;
  onError(error: Error): void;
}) {
  const scope = useWorld();
  const [control, setControl] = useState<GuiControlHandle | null>(null);
  useEffect(() => {
    if (!control || !registration) return;
    let input: ReturnType<HtmlSource["bindInput"]>;
    try {
      input = registration.bindInput(control.target);
    } catch (error) {
      onError(error instanceof Error ? error : new Error(String(error)));
      return;
    }
    let stopped = false;
    let subscription: GuiEffectSubscription | undefined;
    void scope.client.subscribeGuiEffects!(
      (event) => {
        if (!stopped && event.effect.kind === "input")
          input.input(event.effect.input);
      },
      { inputTarget: control.target },
    )
      .then((value) => {
        subscription = value;
        if (stopped) void value.unsubscribe().catch(onError);
        else void value.closed.then(() => input.release());
      })
      .catch((error) => {
        if (!stopped) onError(error);
      });
    return () => {
      stopped = true;
      input.release();
      void subscription?.unsubscribe().catch(onError);
    };
  }, [scope, control, registration, onError]);
  return (
    <Entity id="cockpit-browser-page">
      <Layout width={width} height={height} clip />
      <Behavior enabled={!!registration} />
      <InputTarget ref={setControl} capture_tab capture_escape />
      {registration ? (
        <Html source={registration.source} width={width} height={height} />
      ) : (
        <Children>
          <TextLine id="cockpit-browser-status" text={status} />
        </Children>
      )}
    </Entity>
  );
}
