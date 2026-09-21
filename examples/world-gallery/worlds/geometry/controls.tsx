import type { ChangeEvent } from "react";
import {
  MESH_CATALOG,
  MESH_IDS,
  initialMeshSettings,
  editParameter,
  parameterControl,
  type ViewerShape,
  type MeshSettings,
  type IsolatedShape,
} from "../../shared/geometry-catalog.js";

export interface ControlsState {
  readonly mounted: boolean;
  readonly shape: ViewerShape;
  readonly meshes: Readonly<Record<IsolatedShape, MeshSettings>>;
}

export const INITIAL_CONTROLS: ControlsState = {
  mounted: true,
  shape: "gallery",
  meshes: Object.fromEntries(
    MESH_IDS.map((shape) => [shape, initialMeshSettings(shape)]),
  ) as Record<IsolatedShape, MeshSettings>,
};

export function ShapesControls({
  controls,
  updateControls,
  updateMesh,
}: {
  controls: ControlsState;
  updateControls(change: Partial<ControlsState>): void;
  updateMesh(change: Partial<MeshSettings>): void;
}) {
  const isolated = controls.shape !== "gallery";
  const shape = controls.shape === "gallery" ? "cube" : controls.shape;
  const settings = controls.meshes[shape];
  return (
    <>
      <div className="panel-heading">
        <span className="step">World 01</span>
        <h2>Shape gallery</h2>
        <p>
          Choose a mesh to edit its geometry, finish and transform. Edits are
          saved per mesh.
        </p>
      </div>

      <label className="mesh-select" htmlFor="mesh-select">
        <span>Mesh</span>
        <select
          id="mesh-select"
          value={controls.shape}
          onChange={(event) =>
            updateControls({ shape: event.currentTarget.value as ViewerShape })
          }
        >
          <option value="gallery">All meshes</option>
          {MESH_IDS.map((mesh) => (
            <option key={mesh} value={mesh}>
              {MESH_CATALOG[mesh].label}
            </option>
          ))}
        </select>
      </label>

      <fieldset disabled={!isolated}>
        <legend>Finish</legend>
        <div className="segmented mesh-finish">
          <Choice
            id="finish-solid"
            name="finish"
            label="Solid"
            checked={settings.finish === "solid"}
            onChange={() => updateMesh({ finish: "solid" })}
          />
          <Choice
            id="finish-checker"
            name="finish"
            label="UV grid"
            checked={settings.finish === "checker"}
            onChange={() => updateMesh({ finish: "checker" })}
          />
        </div>
        {!isolated ? (
          <p className="field-note">Choose one shape to apply a finish.</p>
        ) : null}
      </fieldset>

      {isolated && (
        <fieldset className="geometry-parameters">
          <legend>Geometry</legend>
          {MESH_CATALOG[shape].parameters.map((name) => {
            const control = parameterControl(shape, name, settings.parameters);
            const value = settings.parameters[name];
            return (
              <RangeControl
                key={name}
                id={`param-${name}`}
                {...control}
                value={value}
                display={
                  name === "rings"
                    ? String(value)
                    : `${Number(value.toFixed(3))} m`
                }
                disabled={false}
                onChange={(next) =>
                  updateMesh({
                    parameters: editParameter(
                      shape,
                      settings.parameters,
                      name,
                      next,
                    ),
                  })
                }
              />
            );
          })}
          <p className="field-note">
            Dimensions are in metres. Stroke and capsule height adjust to keep
            the geometry valid.
          </p>
          {shape === "axis" &&
            settings.axisColors.map((color, axis) => (
              <label
                key={axis}
                className="color-control"
                htmlFor={`axis-color-${axis}`}
              >
                <span>
                  {["X", "Y", "Z"][axis]} axis color{" "}
                  <small>{color.toUpperCase()}</small>
                </span>
                <input
                  id={`axis-color-${axis}`}
                  type="color"
                  value={color}
                  onChange={(event) => {
                    const colors: [string, string, string] = [
                      ...settings.axisColors,
                    ];
                    colors[axis] = event.currentTarget.value;
                    updateMesh({ axisColors: colors });
                  }}
                />
              </label>
            ))}
        </fieldset>
      )}

      <div className="control-grid">
        <label className="toggle-card">
          <span>
            <strong>World mounted</strong>
            <small>Show or hide the current world</small>
          </span>
          <input
            id="mounted"
            type="checkbox"
            checked={controls.mounted}
            onChange={(event) =>
              updateControls({ mounted: event.currentTarget.checked })
            }
          />
        </label>
        <label className="toggle-card">
          <span>
            <strong>Color override</strong>
            <small>Clear it to reveal component defaults</small>
          </span>
          <input
            id="override"
            type="checkbox"
            checked={settings.override}
            disabled={!isolated}
            onChange={(event) =>
              updateMesh({ override: event.currentTarget.checked })
            }
          />
        </label>
      </div>

      <label className="color-control">
        <span>
          Color <small>{settings.color.toUpperCase()}</small>
        </span>
        <input
          id="color"
          type="color"
          value={settings.color}
          disabled={!isolated || !settings.override}
          onChange={(event) => updateMesh({ color: event.currentTarget.value })}
        />
      </label>

      <RangeControl
        id="position"
        label="Horizontal position"
        value={settings.x}
        display={settings.x.toFixed(2)}
        min={-1.5}
        max={1.5}
        step={0.05}
        disabled={!isolated}
        onChange={(value) => updateMesh({ x: value })}
      />
      <RangeControl
        id="scale"
        label="Scale"
        value={settings.scale}
        display={`${settings.scale.toFixed(2)}×`}
        min={0.25}
        max={1.5}
        step={0.05}
        disabled={!isolated}
        onChange={(value) => updateMesh({ scale: value })}
      />
    </>
  );
}

function Choice({
  id,
  name,
  label,
  checked,
  onChange,
}: {
  readonly id: string;
  readonly name: string;
  readonly label: string;
  readonly checked: boolean;
  readonly onChange: () => void;
}) {
  return (
    <label htmlFor={id}>
      <input
        id={id}
        name={name}
        type="radio"
        checked={checked}
        onChange={onChange}
      />
      <span>{label}</span>
    </label>
  );
}

function RangeControl({
  id,
  label,
  value,
  display,
  min,
  max,
  step,
  disabled,
  onChange,
}: {
  readonly id: string;
  readonly label: string;
  readonly value: number;
  readonly display: string;
  readonly min: number;
  readonly max: number;
  readonly step: number;
  readonly disabled: boolean;
  readonly onChange: (value: number) => void;
}) {
  const handleChange = (event: ChangeEvent<HTMLInputElement>) => {
    onChange(Number(event.currentTarget.value));
  };

  return (
    <label className="range-control" htmlFor={id}>
      <span>
        {label} <small>{display}</small>
      </span>
      <input
        id={id}
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={handleChange}
      />
    </label>
  );
}

export function GeometryLegend({ meshes }: Pick<ControlsState, "meshes">) {
  return (
    <ul className="gallery-key" aria-label="Mesh catalog">
      {MESH_IDS.map((mesh) => (
        <li key={mesh}>
          <span
            className="swatch"
            style={{
              background:
                mesh === "axis"
                  ? `linear-gradient(90deg, ${meshes[mesh].axisColors.join(", ")})`
                  : meshes[mesh].override
                    ? meshes[mesh].color
                    : "#ffffff",
            }}
            aria-hidden="true"
          />
          {MESH_CATALOG[mesh].label}
        </li>
      ))}
    </ul>
  );
}
