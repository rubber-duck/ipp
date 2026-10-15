import {
  SceneChoice,
  SceneSlider,
  sceneActions,
} from "../../shared/control-panel.js";
import type { GallerySceneControlsProps } from "../../shared/scene.js";
import {
  MESH_CATALOG,
  MESH_IDS,
  editParameter,
  parameterControl,
  type ViewerShape,
  type MeshSettings,
} from "../../shared/geometry-catalog.js";
import { type ControlsState } from "./options.js";
export { INITIAL_CONTROLS, type ControlsState } from "./options.js";

export function GeometryControls(props: GallerySceneControlsProps) {
  const state = props.mount.options as unknown as ControlsState;
  const shape = state.shape === "gallery" ? "cube" : state.shape;
  const settings = state.meshes[shape];
  const { update } = sceneActions(props);
  const mesh = (patch: Partial<MeshSettings>) =>
    update({ meshes: { ...state.meshes, [shape]: { ...settings, ...patch } } });
  return (
    <>
      <SceneChoice
        id="geometry-shape"
        label="Shape"
        value={state.shape}
        choices={[
          { key: "gallery", label: "All shapes" },
          ...MESH_IDS.map((key) => ({ key, label: MESH_CATALOG[key].label })),
        ]}
        change={(shape) => update({ shape: shape as ViewerShape })}
      />
      {state.shape !== "gallery" && (
        <>
          <SceneChoice
            id="geometry-finish"
            label="Finish"
            value={settings.finish}
            choices={[
              { key: "solid", label: "Solid" },
              { key: "checker", label: "UV grid" },
            ]}
            change={(finish) =>
              mesh({ finish: finish as MeshSettings["finish"] })
            }
          />
          {MESH_CATALOG[shape].parameters.map((name) => {
            const field = parameterControl(shape, name, settings.parameters);
            return (
              <SceneSlider
                key={name}
                id={`geometry-${name}`}
                label={field.label}
                min={field.min}
                max={field.max}
                step={field.step}
                value={settings.parameters[name]}
                change={(value) =>
                  mesh({
                    parameters: editParameter(
                      shape,
                      settings.parameters,
                      name,
                      value,
                    ),
                  })
                }
              />
            );
          })}
          <SceneChoice
            id="geometry-color"
            label="Color"
            value={settings.color}
            choices={[
              { key: "#ffffff", label: "Porcelain" },
              { key: "#55ddff", label: "Ice blue" },
              { key: "#ffb65a", label: "Amber" },
              { key: "#ed4932", label: "Coral" },
            ]}
            change={(color) => mesh({ color, override: true })}
          />
        </>
      )}
    </>
  );
}
