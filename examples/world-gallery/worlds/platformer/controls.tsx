import type { PlatformerDemo } from "./playback.js";
import type { PlatformerMode } from "./session.js";
export { usePlatformerScene } from "./playback.js";

export function PlatformerControls({ demo }: { demo: PlatformerDemo }) {
  const run = (operation: Promise<void> | undefined) => {
    void operation?.catch((failure) =>
      demo.setError(
        failure instanceof Error ? failure.message : String(failure),
      ),
    );
  };
  return (
    <section aria-label="Platformer controls">
      <div className="panel-heading">
        <span className="step">World 03</span>
        <h2>Trail runner.</h2>
        <p>Choose a gait or reverse along the same Blender-authored route.</p>
      </div>
      <fieldset disabled={!demo.session}>
        <legend>Movement</legend>
        <div className="segmented">
          {(["walk", "run", "crawl"] as const).map((mode) => (
            <label key={mode}>
              <input
                id={`platformer-mode-${mode}`}
                type="radio"
                name="platformer-mode"
                checked={demo.playback.mode === mode}
                onChange={() =>
                  run(demo.session?.setMode(mode as PlatformerMode))
                }
              />
              <span>{mode[0]!.toUpperCase() + mode.slice(1)}</span>
            </label>
          ))}
        </div>
      </fieldset>
      <div className="control-grid platformer-actions">
        <button
          id="platformer-reverse"
          className="secondary-button"
          type="button"
          disabled={!demo.session}
          aria-pressed={demo.playback.direction === -1}
          onClick={() => run(demo.session?.reverse())}
        >
          Reverse
        </button>
        <button
          id="platformer-pause"
          className="secondary-button"
          type="button"
          disabled={!demo.session}
          aria-pressed={!demo.playback.playing}
          onClick={() => run(demo.session?.playback(!demo.playback.playing))}
        >
          {demo.playback.playing ? "Pause" : "Resume"}
        </button>
        <button
          id="platformer-reset"
          className="secondary-button"
          type="button"
          disabled={!demo.session}
          onClick={() => run(demo.session?.reset())}
        >
          Reset route
        </button>
      </div>
      <dl className="selection-summary platformer-summary">
        <div>
          <dt>Direction</dt>
          <dd id="platformer-direction">
            {demo.playback.direction === 1 ? "forward" : "reverse"}
          </dd>
        </div>
      </dl>
      <output
        id="platformer-status"
        data-state={demo.error ? "error" : demo.session ? "ready" : "loading"}
        aria-live="polite"
      >
        {demo.error ?? (demo.session ? "Runner ready" : "Loading runner…")}
      </output>
    </section>
  );
}
