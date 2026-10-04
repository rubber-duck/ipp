import type { SurfaceFacing } from "@ipp/client";
import type {
  GuiPageState,
  GuiScene,
  GuiSurfaceCacheMode,
  GuiSurfaceShape,
} from "./scene.js";
import { sceneNodeLabel } from "./scene-tree.js";
import { onlineCount, operationLabel } from "./station.js";
import { useStoreValue } from "./store.js";
import { hexColor } from "./tuning.js";

interface GuiControlsProps {
  scene: GuiScene;
}

/** The sidebar: its buttons show the settings they toggle, and each readout
 * selects its own text, so a value change re-renders only its readout. */
export function GuiControls({ scene }: GuiControlsProps) {
  const exploded = useStoreValue(scene.state, (state) => state.exploded);
  const vectorOnly = useStoreValue(scene.state, (state) => state.vectorOnly);
  const shieldArmed = useStoreValue(scene.state, (state) => state.shieldArmed);
  const surfaceCache = useStoreValue(
    scene.state,
    (state) => state.surfaceCache,
  );
  const shape = useStoreValue(scene.state, (state) => state.surfaceShape);
  const facing = useStoreValue(scene.state, (state) => state.surfaceFacing);
  return (
    <section aria-label="GUI demo controls">
      <h2>GUI Demo</h2>
      <p>
        Click, drag, scroll, type, or press Tab to operate the dashboard on the
        panel. Drag or zoom outside the panel to inspect it from another angle.
        Reset camera frames the current flat or exploded view.
      </p>
      <label className="mesh-select" htmlFor="gui-surface-shape">
        <span>Panel shape</span>
        <select
          id="gui-surface-shape"
          value={shape}
          disabled={!scene.ready}
          onChange={(event) =>
            scene.selectSurfaceShape(
              event.currentTarget.value as GuiSurfaceShape,
            )
          }
        >
          <option value="flat">Flat</option>
          <option value="cylinder">Cylinder</option>
          <option value="sphere">Sphere</option>
        </select>
      </label>
      <label className="mesh-select" htmlFor="gui-surface-facing">
        <span>Curved panel facing</span>
        <select
          id="gui-surface-facing"
          value={facing}
          disabled={!scene.ready || shape === "flat"}
          onChange={(event) =>
            scene.selectSurfaceFacing(
              event.currentTarget.value as SurfaceFacing,
            )
          }
        >
          <option value="outside">Outside</option>
          <option value="inside">Inside</option>
        </select>
      </label>
      <button
        id="gui-explode-toggle"
        className="secondary-button"
        type="button"
        disabled={!scene.ready}
        aria-pressed={exploded}
        onClick={scene.toggleExplode}
      >
        {exploded ? "Flatten panel layers" : "Explode panel layers"}
      </button>
      <p>
        See how the dashboard is assembled: complete panels separate with their
        labels, borders and controls together. Menus, dialogs and notifications
        float above them. Explode opens a side view; drag the background to
        explore, or Reset camera to frame the stack again. Flatten returns to
        the ordinary view. Curved panels separate into matching curved shells.
      </p>
      <button
        id="gui-vector-only"
        className="secondary-button"
        type="button"
        disabled={!scene.ready}
        aria-pressed={vectorOnly}
        onClick={scene.toggleVectorOnly}
      >
        {vectorOnly ? "Restore full scene" : "Isolate GUI panel only"}
      </button>
      <p>
        Compare the FPS readout at the same camera angle. Isolates the whole GUI
        Surface and camera without projector geometry.
      </p>
      <button
        id="gui-shield-toggle"
        className="secondary-button"
        type="button"
        disabled={!scene.ready || vectorOnly}
        aria-pressed={!shieldArmed}
        onClick={scene.toggleShield}
      >
        {shieldArmed ? "Lift input shield" : "Arm input shield"}
      </button>
      <p>
        The amber cover follows PURGE, with four side walls that block oblique
        clicks while armed. Lifting it greys its frame and lets clicks through.
      </p>
      <label className="mesh-select" htmlFor="gui-surface-cache">
        <span>Panel presentation</span>
        <select
          id="gui-surface-cache"
          value={surfaceCache}
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
        Automatic caching can reuse distant flat panels. Exploded flat panels
        draw directly; curved panels use a separate image for each shell and
        keep interactive controls current.
      </p>
      <dl className="selection-summary">
        {READOUTS.map(([id, label, text]) => (
          <Readout key={id} scene={scene} id={id} label={label} text={text} />
        ))}
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

/** The sidebar's readouts: element id, label and the text each shows. */
const READOUTS: readonly (readonly [
  id: string,
  label: string,
  text: (state: GuiPageState) => string,
])[] = [
  ["gui-accent", "Accent", (state) => state.accent],
  ["gui-layers", "Layers", (state) => (state.exploded ? "exploded" : "flat")],
  ["gui-tab", "Workbench", (state) => state.workbenchTab],
  [
    "gui-focus",
    "Scene focus",
    (state) => sceneNodeLabel(state.tuning.focus) ?? "none",
  ],
  [
    "gui-tuning",
    "Tuning",
    ({ tuning }) =>
      `beam ${tuning.beam}%, light ${tuning.light}%, offset ${tuning.offset}%, sweep ${tuning.sweep.join("-")}%, ${tuning.rate}`,
  ],
  [
    "gui-channels",
    "Channels",
    (state) => state.tuning.channels.join(" ") || "none",
  ],
  [
    "gui-scope",
    "Scope",
    ({ tuning }) => `${tuning.grid}, sweep ${tuning.sweepShown ? "on" : "off"}`,
  ],
  ["gui-colour", "Projection colour", (state) => hexColor(state.tuning.color)],
  ["gui-callsign", "Callsign", (state) => state.callsign || "unassigned"],
  [
    "gui-autoscan",
    "Autoscan",
    (state) => (state.autoscan ? "enabled" : "standby"),
  ],
  ["gui-gain", "Signal gain", (state) => `${Math.round(state.gain * 100)}%`],
  [
    "gui-nodes",
    "Nodes",
    (state) =>
      `${onlineCount(state.nodes, state.gain)} of ${state.nodes.length} online${state.selected ? `, ${state.selected} selected` : ""}`,
  ],
  [
    "gui-operation",
    "Operation",
    ({ operation }) =>
      operation ? `${operationLabel(operation)}: ${operation.phase}` : "none",
  ],
  [
    "gui-shield",
    "Input shield",
    (state) => (state.shieldArmed ? "armed" : "lifted"),
  ],
  [
    "gui-events",
    "Event log",
    (state) =>
      `items ${state.eventWindow.first}-${state.eventWindow.last} of ${state.events.length}`,
  ],
  ["gui-command", "Last command", (state) => state.lastCommand],
];

function Readout({
  scene,
  id,
  label,
  text,
}: {
  scene: GuiScene;
  id: string;
  label: string;
  text: (state: GuiPageState) => string;
}) {
  const value = useStoreValue(scene.state, text);
  return (
    <div>
      <dt>{label}</dt>
      <dd id={id}>{value}</dd>
    </div>
  );
}

export {
  useGuiScene,
  type GuiScene,
  type GuiSurfaceCacheMode,
} from "./scene.js";
