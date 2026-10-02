import type { GuiSceneState, GuiSurfaceCacheMode } from "./scene.js";
import { sceneNodeLabel } from "./scene-tree.js";
import { operationLabel } from "./station.js";
import { hexColor } from "./tuning.js";

interface GuiControlsProps {
  scene: GuiSceneState;
}

export function GuiControls({ scene }: GuiControlsProps) {
  const station = scene.station;
  const operation = station.operation;
  return (
    <section aria-label="GUI demo controls">
      <h2>GUI Demo</h2>
      <p>
        Click, drag, scroll, type, or press Tab to operate the dashboard on the
        panel. Drag or zoom outside the panel to inspect it from another angle.
        Reset camera restores the original view.
      </p>
      <button
        id="gui-explode-toggle"
        className="secondary-button"
        type="button"
        disabled={!scene.ready}
        aria-pressed={scene.exploded}
        onClick={scene.toggleExplode}
      >
        {scene.exploded ? "Flatten panel layers" : "Explode panel layers"}
      </button>
      <p>
        The panels stay whole on the Surface; open overlays lift off it along
        its normal, each on its own plane: lists, popovers, menus and tooltips
        nearest the panels, the PURGE dialog above them and toasts on top. The
        panel stays usable; an exploded panel draws directly, whatever its
        presentation below.
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
      <button
        id="gui-shield-toggle"
        className="secondary-button"
        type="button"
        disabled={!scene.ready || scene.vectorOnly}
        aria-pressed={!scene.shieldArmed}
        onClick={scene.toggleShield}
      >
        {scene.shieldArmed ? "Lift input shield" : "Arm input shield"}
      </button>
      <p>
        The hatched amber shield in front of PURGE is scene geometry with
        picking geometry. Lifting it greys its frame and leaves it in place.
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
        hover and dragging on the panel always draw it directly, and so do
        exploded layers, which one flat image cannot show.
      </p>
      <dl className="selection-summary">
        <div>
          <dt>Accent</dt>
          <dd id="gui-accent">{scene.accent}</dd>
        </div>
        <div>
          <dt>Layers</dt>
          <dd id="gui-layers">{scene.exploded ? "exploded" : "flat"}</dd>
        </div>
        <div>
          <dt>Workbench</dt>
          <dd id="gui-tab">{scene.workbenchTab}</dd>
        </div>
        <div>
          <dt>Scene focus</dt>
          <dd id="gui-focus">
            {sceneNodeLabel(scene.tuning.tuning.focus) ?? "none"}
          </dd>
        </div>
        <div>
          <dt>Tuning</dt>
          <dd id="gui-tuning">
            beam {scene.tuning.tuning.beam}%, light {scene.tuning.tuning.light}
            %, offset {scene.tuning.tuning.offset}%, sweep{" "}
            {scene.tuning.tuning.sweep.join("-")}%, {scene.tuning.tuning.rate}
          </dd>
        </div>
        <div>
          <dt>Channels</dt>
          <dd id="gui-channels">
            {scene.tuning.tuning.channels.join(" ") || "none"}
          </dd>
        </div>
        <div>
          <dt>Scope</dt>
          <dd id="gui-scope">
            {scene.tuning.tuning.grid}, sweep{" "}
            {scene.tuning.tuning.sweepShown ? "on" : "off"}
          </dd>
        </div>
        <div>
          <dt>Projection colour</dt>
          <dd id="gui-colour">{hexColor(scene.tuning.tuning.color)}</dd>
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
          <dt>Signal gain</dt>
          <dd id="gui-gain">{Math.round(scene.gain * 100)}%</dd>
        </div>
        <div>
          <dt>Nodes</dt>
          <dd id="gui-nodes">
            {station.online} of {station.nodes.length} online
            {station.selected ? `, ${station.selected} selected` : ""}
          </dd>
        </div>
        <div>
          <dt>Operation</dt>
          <dd id="gui-operation">
            {operation
              ? `${operationLabel(operation)}: ${operation.phase}`
              : "none"}
          </dd>
        </div>
        <div>
          <dt>Input shield</dt>
          <dd id="gui-shield">{scene.shieldArmed ? "armed" : "lifted"}</dd>
        </div>
        <div>
          <dt>Event log</dt>
          <dd id="gui-events">
            items {scene.eventWindow.first}-{scene.eventWindow.last} of{" "}
            {scene.events.length}
          </dd>
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
