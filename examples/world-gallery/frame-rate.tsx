import type { IppCanvasHandle } from "@ipp/react/web";
import { useEffect, useState } from "react";

type FrameRate =
  | { state: "waiting" | "stale" | "paused" }
  | { state: "live"; fps: number };

const SAMPLE_WINDOW_MS = 1_000;
const PUBLISH_INTERVAL_MS = 500;
const STALE_AFTER_MS = 1_500;

interface FrameSample {
  tick: bigint;
  observedAt: number;
}

/** Observe completed Host frames without polling or rerendering the gallery. */
export function CanvasFrameRate({
  canvas,
}: {
  canvas: IppCanvasHandle | undefined;
}) {
  const [rate, setRate] = useState<FrameRate>({ state: "waiting" });

  useEffect(() => {
    let active = true;
    let lastTick: bigint | undefined;
    let lastFrameAt: number | undefined;
    let samples: FrameSample[] = [];

    const reset = (state: "waiting" | "paused") => {
      lastFrameAt = undefined;
      samples = [];
      setRate({ state });
    };
    const visibilityChanged = () => {
      reset(document.hidden ? "paused" : "waiting");
    };
    const publish = () => {
      if (!active || document.hidden) return;
      const now = performance.now();
      const windowStart = now - SAMPLE_WINDOW_MS;
      const firstInWindow = samples.findIndex(
        (sample) => sample.observedAt >= windowStart,
      );
      if (firstInWindow < 0) samples = samples.slice(-1);
      else if (firstInWindow > 0) samples = samples.slice(firstInWindow - 1);
      if (lastFrameAt === undefined) {
        setRate({ state: "waiting" });
      } else if (now - lastFrameAt > STALE_AFTER_MS) {
        setRate({ state: "stale" });
      } else if (samples.length >= 2) {
        const first = samples[0]!;
        const last = samples.at(-1)!;
        const elapsed = last.observedAt - first.observedAt;
        const frames = Number(last.tick - first.tick);
        if (elapsed > 0)
          setRate({
            state: "live",
            fps: Math.max(1, Math.round((frames * 1_000) / elapsed)),
          });
      } else {
        setRate({ state: "waiting" });
      }
    };
    const observe = async () => {
      while (active && canvas) {
        try {
          const frame = await canvas.client.waitForFrame(lastTick);
          if (!active) return;
          lastTick = frame.tick;
          if (document.hidden) continue;
          const now = performance.now();
          lastFrameAt = now;
          samples.push({ tick: frame.tick, observedAt: now });
        } catch {
          if (!active) return;
          setRate({ state: "stale" });
          await new Promise((resolve) => window.setTimeout(resolve, 250));
        }
      }
    };

    document.addEventListener("visibilitychange", visibilityChanged);
    reset(document.hidden ? "paused" : "waiting");
    const publisher = window.setInterval(publish, PUBLISH_INTERVAL_MS);
    void observe();
    return () => {
      active = false;
      window.clearInterval(publisher);
      document.removeEventListener("visibilitychange", visibilityChanged);
    };
  }, [canvas]);

  const value = rate.state === "live" ? String(rate.fps) : "—";
  return (
    <output
      id="canvas-fps"
      className="canvas-fps"
      data-state={rate.state}
      title="Host frame cadence after world rendering and presentation; this is not a GPU or display refresh measurement"
      aria-live="off"
      aria-label={`Canvas frame rate: ${rate.state === "live" ? `${value} frames per second` : rate.state}`}
    >
      Canvas FPS {value}
    </output>
  );
}
