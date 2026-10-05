/** The demo's local session. All progress is paced by completed Host frames. */
import { useEffect, useMemo, useRef } from "react";
import type { GuiPageState } from "./scene.js";
import type { Store } from "./store.js";
import type { HostFrames } from "./station.js";

export type AppPhase = "login" | "connecting" | "workspace";
export type SettingsPage = "display" | "projection" | "scene";
export interface AppState {
  readonly phase: AppPhase;
  readonly password: string;
  readonly reveal: boolean;
  readonly progress: number;
  readonly lines: readonly string[];
  readonly settings: boolean;
  readonly settingsPage: SettingsPage;
  readonly logOpen: boolean;
  readonly strength: number;
  readonly range: number;
  readonly charge: {
    readonly phase: "idle" | "charging" | "ready";
    readonly progress: number;
  };
}
export const INITIAL_APP: AppState = {
  phase: "login",
  password: "",
  reveal: false,
  progress: 0,
  lines: [],
  settings: false,
  settingsPage: "display",
  logOpen: false,
  strength: 0.75,
  range: 4,
  charge: { phase: "idle", progress: 0 },
};
export const CHARGE_SECONDS = 1.4;
export const LOGIN_SECONDS = 3.6;
export const LOGIN_FADE_SECONDS = 0.4;
const BOOT_LINES = [
  "[local] opening simulation session",
  "[auth ] demo access accepted · no account required",
  "[bus  ] connecting projector interface",
  "[clock] synchronizing with Host",
  "[scan ] mapping the near field",
  "[scan ] calibrating receiver gain",
  "[scan ] rejecting background noise",
  "[beam ] checking projection geometry",
  "[mesh ] curvature compensation online",
  "[guard] pulse interlock armed",
  "[link ] acquiring six simulated contacts",
  "[log  ] recording local telemetry",
  "[scan ] sweep controller ready",
  "[ready] welcome to VESPER",
] as const;

export function useScannerApp(
  state: Store<GuiPageState>,
  frames: HostFrames | undefined,
  ready: boolean,
  reportFailure: (failure: unknown) => void,
  closePulse: () => void,
) {
  const run = useRef(0);
  const charging = useRef(0);
  const latest = useRef({ frames, ready, reportFailure });
  latest.current = { frames, ready, reportFailure };
  useEffect(
    () => () => {
      run.current += 1;
      charging.current += 1;
      state.update(({ app }) => ({
        app: {
          ...app,
          password: "",
          reveal: false,
          charge: { phase: "idle", progress: 0 },
        },
      }));
    },
    [state],
  );
  return useMemo(() => {
    const update = (patch: Partial<AppState>) =>
      state.update(({ app }) => ({ app: { ...app, ...patch } }));
    return {
      setPassword: (password: string) => update({ password }),
      setReveal: (reveal: boolean) => update({ reveal }),
      settings: (settings: boolean) => update({ settings }),
      settingsPage: (settingsPage: SettingsPage) => update({ settingsPage }),
      log: () => update({ logOpen: !state.current.app.logOpen }),
      logout: () => {
        run.current += 1;
        charging.current += 1;
        closePulse();
        state.update({ app: { ...INITIAL_APP }, shieldBlocker: undefined });
      },
      setRange: (range: number) =>
        update({ range: Math.max(1, Math.min(4, range)) }),
      setStrength: (strength: number) => {
        const next = Math.max(0.1, Math.min(1, strength));
        if (next === state.current.app.strength) return;
        charging.current += 1;
        update({ strength: next, charge: { phase: "idle", progress: 0 } });
      },
      consumeCharge: () => {
        const app = state.current.app;
        if (
          app.phase !== "workspace" ||
          app.settings ||
          app.charge.phase !== "ready" ||
          state.current.pulse.state === "running"
        )
          return undefined;
        charging.current += 1;
        update({ charge: { phase: "idle", progress: 0 } });
        return app.strength;
      },
      charge: () => {
        const clock = latest.current.frames;
        const app = state.current.app;
        if (
          !latest.current.ready ||
          !clock ||
          app.phase !== "workspace" ||
          app.settings ||
          app.charge.phase !== "idle" ||
          state.current.pulse.state === "running"
        )
          return;
        const generation = ++charging.current;
        update({ charge: { phase: "charging", progress: 0 } });
        void (async () => {
          const start = (await clock()).time;
          let shown = -1;
          for (;;) {
            const { time } = await clock();
            if (charging.current !== generation) return;
            const progress = Math.min(
              1,
              Math.max(0, (time - start) / CHARGE_SECONDS),
            );
            const step = Math.floor(progress * 100);
            if (step !== shown) {
              shown = step;
              update({
                charge: {
                  phase: progress >= 1 ? "ready" : "charging",
                  progress: step / 100,
                },
              });
            }
            if (progress >= 1) return;
          }
        })().catch((failure: unknown) => {
          if (charging.current === generation)
            latest.current.reportFailure(failure);
        });
      },
      login: () => {
        const clock = latest.current.frames;
        if (
          !latest.current.ready ||
          !clock ||
          state.current.app.phase !== "login"
        )
          return;
        const generation = ++run.current;
        update({
          phase: "connecting",
          password: "",
          reveal: false,
          progress: 0,
          lines: [],
        });
        void (async () => {
          const start = (await clock()).time;
          let shown = -1;
          for (;;) {
            const { time } = await clock();
            if (run.current !== generation) return;
            const progress = Math.min(
              1,
              Math.max(0, (time - start) / LOGIN_SECONDS),
            );
            const step = Math.floor(progress * 100);
            if (step !== shown) {
              shown = step;
              update({
                progress: step / 100,
                lines: BOOT_LINES.slice(
                  0,
                  Math.min(
                    BOOT_LINES.length,
                    1 + Math.floor(progress * BOOT_LINES.length),
                  ),
                ),
              });
            }
            if (progress >= 1) {
              update({ phase: "workspace", progress: 1 });
              return;
            }
          }
        })().catch((failure: unknown) => {
          if (run.current === generation) latest.current.reportFailure(failure);
        });
      },
    };
  }, [state, closePulse]);
}
export type ScannerActions = ReturnType<typeof useScannerApp>;
