import {
  MESH_CATALOG,
  parameterControl,
  editParameter,
  type GeometryParameter,
} from "../../shared/geometry-catalog.js";
import {
  SCENE_OBJECTS,
  isLight,
  type ObjectId,
  type ObjectSettings,
  type LightingWorldObjects,
  type Vec3,
} from "./model.js";

export function LightingControls({
  objects,
  selected,
  select,
  update,
}: {
  objects: LightingWorldObjects;
  selected: ObjectId | undefined;
  select(id: ObjectId | undefined): void;
  update(id: ObjectId, patch: Partial<ObjectSettings>): void;
}) {
  const object = SCENE_OBJECTS.find((object) => object.id === selected);
  const state = selected ? objects[selected] : undefined;
  const light = selected !== undefined && isLight(selected);
  const change = (patch: Partial<ObjectSettings>) => {
    if (selected) update(selected, patch);
  };
  return (
    <>
      <div className="panel-heading">
        <span className="step">World 02</span>
        <h2>Lighting, Picking & Animation</h2>
        <p>
          Pick an object to edit it. Drag an object to move it; drag empty space
          to orbit.
        </p>
      </div>
      <label className="mesh-select-label" htmlFor="object-select">
        Selected object
      </label>
      <select
        id="object-select"
        value={selected ?? ""}
        onChange={(event) =>
          select(
            event.currentTarget.value
              ? (event.currentTarget.value as ObjectId)
              : undefined,
          )
        }
      >
        <option value="">Select an object…</option>
        {SCENE_OBJECTS.map((object) => (
          <option key={object.id} value={object.id}>
            {object.name}
          </option>
        ))}
      </select>
      {object && state && (
        <div className="object-inspector" data-object={selected}>
          <h3>{object.name}</h3>
          <fieldset>
            <legend>Position</legend>
            <div className="position-fields">
              {(["X", "Y", "Z"] as const).map((label, axis) => (
                <label key={label}>
                  {label}
                  <input
                    id={`object-position-${label.toLowerCase()}`}
                    type="number"
                    min={-10}
                    max={10}
                    step={0.1}
                    value={Number(state.position[axis]!.toFixed(3))}
                    onChange={(event) => {
                      const value = event.currentTarget.valueAsNumber;
                      if (!Number.isFinite(value)) return;
                      const position = [...state.position] as Vec3;
                      position[axis] = Math.max(-10, Math.min(10, value));
                      change({ position });
                    }}
                  />
                </label>
              ))}
            </div>
          </fieldset>
          <Slider
            id="object-scale"
            label="Scale"
            value={state.scale}
            min={0.25}
            max={2}
            step={0.05}
            change={(scale) => change({ scale })}
          />
          <label className="color-control">
            <span>
              {light ? "Light color" : "Base color"}
              <small>{state.color.toUpperCase()}</small>
            </span>
            <input
              id="object-color"
              type="color"
              value={state.color}
              onChange={(event) => change({ color: event.currentTarget.value })}
            />
          </label>
          {light ? (
            <>
              <Slider
                id="object-intensity"
                label="Intensity"
                value={state.intensity}
                min={0}
                max={selected === "lighting-fill" ? 3 : 160}
                step={selected === "lighting-fill" ? 0.05 : 1}
                change={(intensity) => change({ intensity })}
              />
              {selected !== "lighting-fill" && (
                <Slider
                  id="object-range"
                  label="Range"
                  value={state.range}
                  min={1}
                  max={30}
                  step={0.5}
                  change={(range) => change({ range })}
                />
              )}
              <Slider
                id="object-marker-size"
                label="Marker size"
                value={state.markerSize}
                min={0.5}
                max={2}
                step={0.05}
                change={(markerSize) => change({ markerSize })}
              />
              {selected === "lighting-spot" && (
                <>
                  <Slider
                    id="object-inner-cone"
                    label="Inner angle"
                    value={state.innerCone}
                    min={0}
                    max={1.4}
                    step={0.01}
                    change={(innerCone) =>
                      change({
                        innerCone,
                        outerCone: Math.max(state.outerCone, innerCone + 0.02),
                      })
                    }
                  />
                  <Slider
                    id="object-outer-cone"
                    label="Outer angle"
                    value={state.outerCone}
                    min={0.05}
                    max={1.5}
                    step={0.01}
                    change={(outerCone) =>
                      change({
                        outerCone,
                        innerCone: Math.min(state.innerCone, outerCone - 0.02),
                      })
                    }
                  />
                  <Toggle
                    id="object-cast-shadows"
                    label="Cast shadows"
                    checked={state.castShadows}
                    change={(castShadows) => change({ castShadows })}
                  />
                </>
              )}
            </>
          ) : (
            <>
              <fieldset>
                <legend>Mesh parameters</legend>
                {(
                  MESH_CATALOG[object.shape]
                    .parameters as readonly GeometryParameter[]
                ).map((name) => {
                  const control = parameterControl(
                    object.shape,
                    name,
                    state.parameters,
                  );
                  return (
                    <Slider
                      key={name}
                      id={`object-param-${name}`}
                      label={control.label}
                      value={state.parameters[name]}
                      min={control.min}
                      max={control.max}
                      step={control.step}
                      change={(value) =>
                        change({
                          parameters: editParameter(
                            object.shape,
                            state.parameters,
                            name,
                            value,
                          ),
                        })
                      }
                    />
                  );
                })}
              </fieldset>
              <Slider
                id="object-roughness"
                label="Roughness"
                value={state.roughness}
                min={0.05}
                max={1}
                step={0.01}
                change={(roughness) => change({ roughness })}
              />
              <Slider
                id="object-metallic"
                label="Metallic"
                value={state.metallic}
                min={0}
                max={1}
                step={0.01}
                change={(metallic) => change({ metallic })}
              />
              <Toggle
                id="object-cast-shadows"
                label="Cast shadows"
                checked={state.castShadows}
                change={(castShadows) => change({ castShadows })}
              />
              <Toggle
                id="object-receive-shadows"
                label="Receive shadows"
                checked={state.receiveShadows}
                change={(receiveShadows) => change({ receiveShadows })}
              />
            </>
          )}
        </div>
      )}
    </>
  );
}

function Slider({
  id,
  label,
  value,
  min,
  max,
  step,
  change,
}: {
  id: string;
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  change(value: number): void;
}) {
  return (
    <label className="lighting-slider">
      <span>
        {label}
        <output>{value.toFixed(step >= 1 ? 0 : 2)}</output>
      </span>
      <input
        id={id}
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(event) => change(event.currentTarget.valueAsNumber)}
      />
    </label>
  );
}
function Toggle({
  id,
  label,
  checked,
  change,
}: {
  id: string;
  label: string;
  checked: boolean;
  change(value: boolean): void;
}) {
  return (
    <label className="toggle-card">
      <span>
        <strong>{label}</strong>
      </span>
      <input
        id={id}
        type="checkbox"
        checked={checked}
        onChange={(event) => change(event.currentTarget.checked)}
      />
    </label>
  );
}

export function SelectionSummary({
  selected,
  pending,
}: {
  selected: ObjectId | undefined;
  pending: number;
}) {
  return (
    <div className="selection-summary" aria-live="polite">
      <p
        id="selection-status"
        data-pending={pending}
        data-selected={selected ?? ""}
      >
        {selected
          ? `${SCENE_OBJECTS.find((object) => object.id === selected)!.name} selected`
          : "No object selected"}
      </p>
      <p>Drag empty space to orbit. Scroll to zoom.</p>
    </div>
  );
}
