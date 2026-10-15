import type { AnimationWorldClient } from "@ipp/client";
import type { IppCanvasHandle } from "@ipp/react/canvas";
import { useEffect, useState } from "react";
import { AnimationSession, type DemoPlayer } from "./animation-controller.js";
import type { AnimationModule } from "./animation-assets.js";

/** React owns the page lifetime; the Rust player owns time and evaluated motion. */
export function useWorldAnimation(
  canvas: IppCanvasHandle | undefined,
  active: boolean,
  contract: Readonly<Record<string, unknown>>,
) {
  const [session, setSession] = useState<AnimationSession>();
  const [players, setPlayers] = useState<DemoPlayer[]>([]);
  const [error, setError] = useState<string>();

  useEffect(() => {
    setSession(undefined);
    setPlayers([]);
    setError(undefined);
    if (!canvas || !active) return;
    const controller = new AbortController();
    let owned: AnimationSession | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let unsubscribe: (() => void) | undefined;
    const report = (failure: unknown) => {
      if (!controller.signal.aborted)
        setError(failure instanceof Error ? failure.message : String(failure));
    };
    void (async () => {
      const client = canvas.client as AnimationWorldClient;
      owned = await AnimationSession.create(
        canvas,
        contract as unknown as AnimationModule,
        controller.signal,
      );
      if (controller.signal.aborted) {
        await owned.close();
        return;
      }
      const current = owned;
      unsubscribe = client.onPlaybackEvent((event) => {
        if (
          current.players.includes(event.controller.id) &&
          (event.kind === "failed" || event.kind === "invalidated")
        )
          report(new Error(event.reason ?? "Playback failed"));
      });
      const observe = async () => {
        try {
          const snapshot = await current.observe();
          if (controller.signal.aborted) return;
          setPlayers(snapshot);
          setSession(current);
          // Observation only: this timer never advances simulation or authors a pose.
          timer = setTimeout(() => {
            void observe();
          }, 120);
        } catch (failure) {
          report(failure);
        }
      };
      await observe();
    })().catch(report);
    return () => {
      controller.abort();
      clearTimeout(timer);
      unsubscribe?.();
      void owned?.close().catch(report);
    };
  }, [canvas, active, contract]);

  return { session, players, error };
}

export type WorldAnimationDemo = ReturnType<typeof useWorldAnimation>;
