import type {
  AnimationWorldClient,
  AnimationPlaybackControl,
} from "@ipp/client";
import type { IppCanvasHandle } from "@ipp/react/web";
import { useEffect, useState } from "react";
import { ANIMATION_DURATION } from "./animation-assets.js";
import { AnimationSession, type DemoPlayer } from "./animation-controller.js";
import { isAnimated, type ObjectId } from "./model.js";

import { GALLERY_RUNTIME } from "../../shared/runtime.js";

/** React owns the page lifetime; the Rust player owns time and evaluated motion. */
export function useWorldAnimation(
  canvas: IppCanvasHandle | undefined,
  active: boolean,
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
        `${GALLERY_RUNTIME}generated.js`,
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
  }, [canvas, active]);

  return { session, players, error };
}

export function AnimationControls({
  demo,
  selectedObject,
}: {
  demo: ReturnType<typeof useWorldAnimation>;
  selectedObject?: ObjectId | undefined;
}) {
  const [selection, setSelection] = useState("all");
  useEffect(() => {
    setSelection(
      selectedObject && isAnimated(selectedObject) ? selectedObject : "all",
    );
  }, [selectedObject]);
  const [error, setError] = useState<string>();
  const selected = demo.players.filter(
    (player) => selection === "all" || player.symbol === selection,
  );
  const first = selected[0];
  const [cursor, setCursor] = useState(0);
  useEffect(() => {
    setCursor(first?.time ?? 0);
  }, [first?.id, first?.time]);
  const speed = selected.every((player) => player.speed === first?.speed)
    ? String(first?.speed ?? 1)
    : "mixed";
  const looping = selected.every((player) => player.looping);
  const [displaySpeed, setDisplaySpeed] = useState("1");
  const [displayLooping, setDisplayLooping] = useState(true);
  useEffect(() => {
    setDisplaySpeed(speed);
  }, [speed, selection]);
  useEffect(() => {
    setDisplayLooping(looping);
  }, [looping, selection]);
  const run = (action: () => void | Promise<void>) => {
    setError(undefined);
    try {
      void Promise.resolve(action()).catch((failure) =>
        setError(String(failure)),
      );
    } catch (failure) {
      setError(String(failure));
    }
  };
  const control = (control: AnimationPlaybackControl) =>
    run(() => demo.session?.control(selection, control));

  return (
    <>
      <h3>Animation</h3>
      <fieldset disabled={!demo.session}>
        <legend>Playback target</legend>
        <select
          id="animation-target"
          value={selection}
          onChange={(event) => setSelection(event.currentTarget.value)}
        >
          <option value="all">All animations</option>
          {demo.players.map((player) => (
            <option key={player.symbol} value={player.symbol}>
              {player.name}
            </option>
          ))}
        </select>
        <div className="playback-states">
          {selected.map((player) => (
            <small
              key={player.symbol}
              data-player={player.symbol}
              data-playback-state={player.state}
            >
              {player.name}: {player.state} · {player.speed}×
            </small>
          ))}
        </div>
      </fieldset>
      <fieldset className="animation-transport" disabled={!demo.session}>
        <legend>Playback</legend>
        <div className="animation-buttons">
          {(["play", "pause", "stop", "restart"] as const).map((action) => (
            <button
              key={action}
              id={`animation-${action}`}
              type="button"
              className="secondary-button"
              onClick={() => control({ action })}
            >
              {action[0]!.toUpperCase() + action.slice(1)}
            </button>
          ))}
        </div>
        <label className="animation-slider">
          <span>
            Seek{" "}
            <output>
              {cursor.toFixed(2)} / {ANIMATION_DURATION.toFixed(2)} s
            </output>
          </span>
          <input
            id="animation-seek"
            type="range"
            min="0"
            max={ANIMATION_DURATION}
            step="0.01"
            value={cursor}
            onChange={(event) => {
              const time = event.currentTarget.valueAsNumber;
              // Keep the controlled input current while the runtime processes the seek.
              setCursor(time);
              run(() => {
                demo.session?.control(selection, { action: "play" });
                demo.session?.control(selection, { action: "pause" });
                demo.session?.control(selection, { action: "seek", time });
              });
            }}
          />
        </label>
        <p className="animation-note">
          Scrub to pause at an exact pose. Stop rewinds to the starting pose.
          Play continues from there; Restart plays from zero.
        </p>
        <label className="animation-speed">
          Speed
          <select
            id="animation-speed"
            value={displaySpeed}
            onChange={(event) => {
              const value = Number(event.currentTarget.value);
              setDisplaySpeed(String(value));
              run(async () => {
                try {
                  await demo.session?.configure(selection, "speed", value);
                } catch (failure) {
                  setDisplaySpeed(speed);
                  throw failure;
                }
              });
            }}
          >
            <option value="mixed" disabled>
              Mixed
            </option>
            {[0.25, 0.5, 1, 2].map((value) => (
              <option key={value} value={value}>
                {value}×
              </option>
            ))}
          </select>
        </label>
        <label className="toggle-card">
          <span>
            <strong>Loop</strong>
            <small>Repeat the eight-second motion</small>
          </span>
          <input
            id="animation-loop"
            type="checkbox"
            checked={displayLooping}
            onChange={(event) => {
              const value = event.currentTarget.checked;
              setDisplayLooping(value);
              run(async () => {
                try {
                  await demo.session?.configure(selection, "looping", value);
                } catch (failure) {
                  setDisplayLooping(looping);
                  throw failure;
                }
              });
            }}
          />
        </label>
      </fieldset>
      <p className="animation-note">
        The spotlight moves toward the center, the point light orbits the Y
        axis, the sun changes direction, and the solid beam bends at its joint.
        Select one to control its playback independently.
      </p>
      {error && <p role="alert">{error}</p>}
    </>
  );
}
