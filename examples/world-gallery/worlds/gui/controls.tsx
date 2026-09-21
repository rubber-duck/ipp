import type { GuiSceneState } from "./scene.js";

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

export { useGuiScene, type GuiSceneState } from "./scene.js";
