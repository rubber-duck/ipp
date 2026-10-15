import {
  SceneChoice,
  SceneSlider,
  SceneButtons,
  sceneActions,
} from "../../shared/control-panel.js";
import type { GallerySceneControlsProps } from "../../shared/scene.js";
import {
  SCENE_OBJECTS,
  isLight,
  type LightingWorldObjects,
  type ObjectId,
} from "./model.js";
import { ANIMATION_DURATION } from "./animation-assets.js";
export function LightingControls(props: GallerySceneControlsProps) {
  const objects = props.mount.options.objects as LightingWorldObjects;
  const selected = props.mount.options.selected as ObjectId | undefined;
  const settings = selected ? objects[selected] : undefined;
  const { action } = sceneActions(props);
  const change = (patch: object) =>
    action("updateObject", { id: selected, patch });
  return (
    <>
      <SceneChoice
        id="lighting-object"
        label="Object"
        value={selected ?? ""}
        choices={[
          { key: "", label: "Pick an object in the scene" },
          ...SCENE_OBJECTS.map((object) => ({
            key: object.id,
            label: object.name,
          })),
        ]}
        change={(id) => action("select", id || undefined)}
      />
      {settings && selected && (
        <>
          {isLight(selected) ? (
            <SceneSlider
              id="lighting-intensity"
              label="Light energy"
              value={settings.intensity}
              min={0}
              max={selected === "lighting-fill" ? 3 : 160}
              step={selected === "lighting-fill" ? 0.05 : 1}
              change={(intensity) => change({ intensity })}
            />
          ) : (
            <>
              <SceneSlider
                id="lighting-roughness"
                label="Roughness"
                value={settings.roughness}
                min={0.05}
                max={1}
                change={(roughness) => change({ roughness })}
              />
              <SceneSlider
                id="lighting-metallic"
                label="Metal"
                value={settings.metallic}
                min={0}
                max={1}
                change={(metallic) => change({ metallic })}
              />
            </>
          )}
          <SceneChoice
            id="lighting-color"
            label="Color"
            value={settings.color}
            choices={[
              { key: "#ffffff", label: "White" },
              { key: "#55ddff", label: "Blue" },
              { key: "#ffb65a", label: "Warm amber" },
            ]}
            change={(color) => change({ color })}
          />
        </>
      )}
      <SceneButtons
        buttons={[
          { id: "lighting-play", label: "Play", press: () => action("play") },
          {
            id: "lighting-pause",
            label: "Pause",
            press: () => action("pause"),
          },
        ]}
      />
      <SceneSlider
        id="lighting-seek"
        label="Pose"
        value={Number(
          (
            props.mount.options.animations as
              | Record<string, { time?: number }>
              | undefined
          )?.["lighting-point"]?.time ?? 0,
        )}
        min={0}
        max={ANIMATION_DURATION}
        step={0.05}
        change={(time) => action("seek", { time })}
      />
    </>
  );
}
