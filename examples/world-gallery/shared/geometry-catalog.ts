import { hexToLinear } from "./colors.js";

export interface GeometryParameters {
  readonly width: number;
  readonly height: number;
  readonly length: number;
  readonly radius: number;
  readonly size: number;
  readonly normalLength: number;
  readonly normalOffset: number;
  readonly stroke: number;
  readonly rings: number;
}

export type GeometryParameter = keyof GeometryParameters;

interface MeshDefinition {
  readonly label: string;
  readonly recipe: string;
  readonly parameters: readonly GeometryParameter[];
  readonly defaults: Partial<GeometryParameters>;
  readonly color: string;
}

export const MESH_CATALOG = {
  cube: {
    label: "Cube",
    recipe: "cube",
    parameters: ["width", "height", "length"],
    defaults: {},
    color: "#65e887",
  },
  sphere: {
    label: "Sphere",
    recipe: "sphere",
    parameters: ["radius"],
    defaults: {},
    color: "#ff6c7c",
  },
  pill: {
    label: "Pill",
    recipe: "pill",
    parameters: ["radius", "height"],
    defaults: { radius: 0.65, height: 2.8 },
    color: "#a0e85f",
  },
  plane: {
    label: "Plane + normal",
    recipe: "plane",
    parameters: ["size", "normalLength", "stroke", "normalOffset"],
    defaults: { stroke: 0.05 },
    color: "#fa95db",
  },
  cubeOutline: {
    label: "Cube outline",
    recipe: "cube-outline",
    parameters: ["width", "height", "length", "stroke"],
    defaults: {},
    color: "#7093ff",
  },
  sphereOutline: {
    label: "Sphere outline",
    recipe: "sphere-outline",
    parameters: ["radius", "stroke"],
    defaults: {},
    color: "#ffc951",
  },
  pillOutline: {
    label: "Pill outline",
    recipe: "pill-outline",
    parameters: ["radius", "height", "stroke"],
    defaults: { radius: 0.65, height: 2.8 },
    color: "#bd8eff",
  },
  planeOutline: {
    label: "Plane outline + normal",
    recipe: "plane-outline",
    parameters: ["size", "normalLength", "stroke", "normalOffset"],
    defaults: { stroke: 0.05 },
    color: "#6cedca",
  },
  cone: {
    label: "Cone",
    recipe: "cone",
    parameters: ["radius", "height"],
    defaults: {},
    color: "#ffb975",
  },
  coneOutline: {
    label: "Cone outline",
    recipe: "cone-outline",
    parameters: ["radius", "height", "stroke", "rings"],
    defaults: {},
    color: "#80d9ff",
  },
  arrow: {
    label: "Arrow",
    recipe: "arrow",
    parameters: ["length", "stroke"],
    defaults: { length: 1.25, stroke: 0.05 },
    color: "#ffffff",
  },
  axis: {
    label: "Axis",
    recipe: "axis",
    parameters: ["length", "stroke"],
    defaults: { length: 1.25, stroke: 0.05 },
    color: "#ffffff",
  },
} as const satisfies Record<string, MeshDefinition>;

export type IsolatedShape = keyof typeof MESH_CATALOG;
export type ViewerShape = "gallery" | IsolatedShape;
export type ViewerFinish = "solid" | "checker";
export const MESH_IDS = Object.keys(MESH_CATALOG) as IsolatedShape[];

export interface MeshSettings {
  readonly finish: ViewerFinish;
  readonly override: boolean;
  readonly color: string;
  readonly x: number;
  readonly scale: number;
  readonly parameters: GeometryParameters;
  readonly axisColors: readonly [string, string, string];
}

export function initialMeshSettings(shape: IsolatedShape): MeshSettings {
  return {
    finish:
      shape.endsWith("Outline") || shape === "arrow" || shape === "axis"
        ? "solid"
        : "checker",
    override: shape !== "axis",
    color: MESH_CATALOG[shape].color,
    x: 0,
    scale: 1,
    parameters: {
      width: 2,
      height: 2,
      length: 2,
      radius: 1,
      size: 2,
      normalLength: 1.25,
      normalOffset: 0.1,
      stroke: 0.045,
      rings: 1,
      ...MESH_CATALOG[shape].defaults,
    },
    axisColors: ["#ff0000", "#00ff00", "#0000ff"],
  };
}

export function meshSource(
  shape: IsolatedShape,
  settings: MeshSettings,
): string {
  const definition = MESH_CATALOG[shape];
  const pairs = definition.parameters.map(
    (name) => `${name}=${settings.parameters[name]}`,
  );
  if (shape === "axis") {
    settings.axisColors.forEach((color, axis) => {
      // Six decimal places preserve picker precision and keep the recipe bounded.
      const rgb = hexToLinear(color)
        .map((value) => Number(value.toFixed(6)))
        .join(",");
      pairs.push(`${["xColor", "yColor", "zColor"][axis]}=${rgb}`);
    });
  }
  return `ipp://mesh/${definition.recipe}?${pairs.join("&")}`;
}

export const VIEWER_MESH_SOURCES = Object.fromEntries(
  MESH_IDS.map((shape) => [
    shape,
    meshSource(shape, initialMeshSettings(shape)),
  ]),
) as Record<IsolatedShape, string>;

const PARAMETER_CONTROLS: Record<
  GeometryParameter,
  { label: string; min: number; max: number; step: number }
> = {
  width: { label: "Width", min: 0.25, max: 3, step: 0.05 },
  height: { label: "Height", min: 0.25, max: 4, step: 0.05 },
  length: { label: "Length", min: 0.25, max: 3, step: 0.05 },
  radius: { label: "Radius", min: 0.2, max: 1.5, step: 0.05 },
  size: { label: "Square size", min: 0.25, max: 3, step: 0.05 },
  normalLength: { label: "Normal length", min: 0.25, max: 2.5, step: 0.05 },
  normalOffset: { label: "Normal offset", min: 0, max: 1, step: 0.05 },
  stroke: { label: "Stroke diameter", min: 0.005, max: 0.25, step: 0.001 },
  rings: { label: "Interior rings", min: 0, max: 16, step: 1 },
};

export function parameterControl(
  shape: IsolatedShape,
  name: GeometryParameter,
  p: GeometryParameters,
) {
  const control = PARAMETER_CONTROLS[name];
  if (name === "height" && (shape === "pill" || shape === "pillOutline")) {
    return { ...control, min: 2 * p.radius };
  }
  if (name !== "stroke") return control;
  let max = control.max;
  switch (shape) {
    case "cubeOutline":
      max = Math.min(max, p.width / 4, p.height / 4, p.length / 4);
      break;
    case "sphereOutline":
      max = Math.min(max, p.radius / 2);
      break;
    case "pillOutline":
      max = Math.min(max, p.radius / 2, p.height / 4);
      break;
    case "coneOutline":
      max = Math.min(max, p.radius / 2, p.height / 4);
      if (p.rings > 0)
        max = Math.min(
          max,
          p.radius / (p.rings + 1),
          p.height / (2 * (p.rings + 1)),
        );
      break;
    case "plane":
    case "planeOutline":
      max = Math.min(max, p.size / 8, p.normalLength / 8);
      break;
    case "arrow":
    case "axis":
      max = Math.min(max, p.length / 8);
      break;
  }
  return { ...control, max: Math.floor(max * 1000) / 1000 };
}

/** Keep coupled dimensions valid in the same React update as the edited value. */
export function editParameter(
  shape: IsolatedShape,
  current: GeometryParameters,
  name: GeometryParameter,
  value: number,
): GeometryParameters {
  if (!Number.isFinite(value)) return current;
  const { min, max } = parameterControl(shape, name, current);
  const parameters = {
    ...current,
    [name]: Math.max(min, Math.min(max, value)),
  };
  if (shape === "pill" || shape === "pillOutline") {
    parameters.height = Math.max(parameters.height, 2 * parameters.radius);
  }
  const stroke = parameterControl(shape, "stroke", parameters);
  parameters.stroke = Math.min(parameters.stroke, stroke.max);
  return parameters;
}
