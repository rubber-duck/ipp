import type { GuiSceneState, GuiSurfaceCacheMode } from "./scene.js";

interface GuiControlsProps {
  scene: GuiSceneState;
}

export function GuiControls({ scene }: GuiControlsProps) {
  return (
    <section aria-label="GUI demo controls">
      <h2>GUI Demo</h2>
      <p>
        Click, drag, scroll, type, or press Tab to operate the controls on the
        panel. Drag or zoom outside the panel to inspect the demo from another
        angle. Reset camera restores the original view.
      </p>
      <button
        id="gui-vector-only"
        className="secondary-button"
        type="button"
        disabled={!scene.ready}
        aria-pressed={scene.vectorOnly}
        onClick={scene.toggleVectorOnly}
      >
        {scene.vectorOnly ? "Restore full scene" : "Isolate GUI panel only"}
      </button>
      <p>
        Compare the FPS readout at the same camera angle. Isolates the whole GUI
        Surface and camera without projector geometry.
      </p>
      <label className="mesh-select" htmlFor="gui-surface-cache">
        <span>Panel presentation</span>
        <select
          id="gui-surface-cache"
          value={scene.surfaceCache}
          disabled={!scene.ready}
          onChange={(event) =>
            scene.selectSurfaceCache(
              event.currentTarget.value as GuiSurfaceCacheMode,
            )
          }
        >
          <option value="automatic">Automatic caching</option>
          <option value="cached">Cached at every distance</option>
          <option value="direct">Direct rendering</option>
        </select>
      </label>
      <p>
        Automatic caching draws the panel directly near the authored view and
        from a reduced-rate texture once the camera zooms out past 22 m. Focus,
        hover and dragging on the panel always draw it directly.
      </p>
      <dl className="selection-summary">
        <div>
          <dt>Skin</dt>
          <dd id="gui-skin">{scene.skin}</dd>
        </div>
        <div>
          <dt>Callsign</dt>
          <dd id="gui-callsign">{scene.callsign || "unassigned"}</dd>
        </div>
        <div>
          <dt>Autoscan</dt>
          <dd id="gui-autoscan">{scene.autoscan ? "enabled" : "standby"}</dd>
        </div>
        <div>
          <dt>Span</dt>
          <dd id="gui-span">{scene.wide ? "wide" : "narrow"}</dd>
        </div>
        <div>
          <dt>Signal gain</dt>
          <dd id="gui-gain">{Math.round(scene.gain * 100)}%</dd>
        </div>
        <div>
          <dt>Last command</dt>
          <dd id="gui-command">{scene.lastCommand}</dd>
        </div>
        <div>
          <dt>Status</dt>
          <dd id="gui-status">
            {scene.error ? "error" : scene.ready ? "ready" : "loading"}
          </dd>
        </div>
      </dl>
      {scene.error && <p role="alert">{scene.error}</p>}
    </section>
  );
}

export {
  useGuiScene,
  type GuiSceneState,
  type GuiSurfaceCacheMode,
} from "./scene.js";
