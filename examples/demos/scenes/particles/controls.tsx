import {
  SceneChoice,
  SceneSlider,
  SceneButtons,
  sceneActions,
} from "../../shared/control-panel.js";
import type { GallerySceneControlsProps } from "../../shared/scene.js";
import type { ParticleSettings } from "./options.js";
export { INITIAL_PARTICLES, type ParticleSettings } from "./options.js";
const PRESETS = {
  fountain: { rate: 900, lifetime: 2.2, speed: 5, spread: 0.45, size: 0.09 },
  embers: { rate: 260, lifetime: 3.5, speed: 2.5, spread: 0.8, size: 0.13 },
  storm: { rate: 1600, lifetime: 1.5, speed: 7, spread: 1.1, size: 0.07 },
};
export function ParticleControls(props: GallerySceneControlsProps) {
  const settings = props.mount.options as unknown as ParticleSettings;
  const { action, update } = sceneActions(props);
  return (
    <>
      <SceneChoice
        id="particles-presentation"
        label="Appearance"
        value={settings.presentation}
        choices={[
          { key: "sprites", label: "Glowing sprites" },
          { key: "meshes", label: "Tumbling cubes" },
        ]}
        change={(presentation) => update({ presentation })}
      />
      <SceneChoice
        id="particles-preset"
        label="Spray"
        value={String(props.mount.options.preset ?? "fountain")}
        choices={[
          { key: "fountain", label: "Fountain" },
          { key: "embers", label: "Drifting embers" },
          { key: "storm", label: "Storm" },
        ]}
        change={(preset) =>
          update({
            preset,
            ...PRESETS[preset as keyof typeof PRESETS],
            restart: settings.restart + 1,
          })
        }
      />
      <SceneSlider
        id="particles-spread"
        label="Spread"
        value={settings.spread}
        min={0.05}
        max={1.2}
        change={(spread) => update({ spread })}
      />
      <SceneChoice
        id="particles-color"
        label="Color"
        value={settings.color}
        choices={[
          { key: "#55ddff", label: "Ion blue" },
          { key: "#ff5500", label: "Ember" },
          { key: "#ff88dd", label: "Plasma" },
        ]}
        change={(color) => update({ color })}
      />
      <SceneButtons
        buttons={[
          {
            id: "particles-emission",
            label: settings.emitting ? "Stop emission" : "Emit",
            press: () => action("toggleEmission"),
          },
          {
            id: "particles-restart",
            label: "Restart spray",
            press: () => action("restart"),
          },
        ]}
      />
    </>
  );
}
