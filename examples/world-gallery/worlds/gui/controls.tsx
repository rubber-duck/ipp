import type { GuiScene } from "./scene.js";

/** Camera guidance remains beside the viewer; all demo controls live in its canvas. */
export function GuiControls({ scene }: { scene: GuiScene }) {
  return (
    <section aria-label="GUI camera guidance">
      <h2>VESPER Scanner</h2>
      <p>
        Log in to the local scanner in the scene. SETTINGS holds its projection,
        display and scene inspection controls. Drag or zoom the background to
        inspect them; Reset camera frames the current view.
      </p>
      {scene.error && <p role="alert">{scene.error}</p>}
    </section>
  );
}

export {
  useGuiScene,
  type GuiScene,
  type GuiSurfaceCacheMode,
} from "./scene.js";
