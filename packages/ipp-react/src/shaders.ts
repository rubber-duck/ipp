import { createElement } from "react";
import {
  type ShaderParameterKind,
  type ShaderRecipe,
  type ShaderDefinition,
} from "@ipp/client";

export const VERTEX_SHADER_HOST_TYPE = "ipp-vertex-shader";
export const FRAGMENT_SHADER_HOST_TYPE = "ipp-fragment-shader";
export type ShaderHostType =
  | typeof VERTEX_SHADER_HOST_TYPE
  | typeof FRAGMENT_SHADER_HOST_TYPE;

export interface ShaderProps {
  /** Backend-specific source, including the materialVertex/materialFragment entry. */
  children: string;
  /** Comma-separated material property names, or a list of names. */
  references?: string | readonly string[];
  /** GLSL ES 3.00 supports WebGL 2 and GLES 3. Other entries are preserved. */
  backend?: string;
  /** Bit 0 color, bit 1 UV, bit 2 normal, bit 3 texture weight. */
  requiredAttributes?: number;
}

export function VertexShader(props: ShaderProps) {
  return createElement(VERTEX_SHADER_HOST_TYPE, props);
}

export function FragmentShader(props: ShaderProps) {
  return createElement(FRAGMENT_SHADER_HOST_TYPE, props);
}

export function isShader(type: string): type is ShaderHostType {
  return type === VERTEX_SHADER_HOST_TYPE || type === FRAGMENT_SHADER_HOST_TYPE;
}

export function shaderProps(
  props: Readonly<Record<string, unknown>>,
): ShaderProps {
  for (const key of Object.keys(props))
    if (
      !["children", "references", "backend", "requiredAttributes"].includes(key)
    )
      throw new Error(`Unsupported shader prop: ${key}`);
  if (typeof props.children !== "string")
    throw new Error("Shader children must be a source string");
  if (
    props.backend !== undefined &&
    (typeof props.backend !== "string" || !props.backend)
  )
    throw new Error("Shader backend must be a nonempty string");
  if (
    props.references !== undefined &&
    typeof props.references !== "string" &&
    (!Array.isArray(props.references) ||
      props.references.some((name) => typeof name !== "string"))
  )
    throw new Error(
      "Shader references must be names or a comma-separated string",
    );
  if (
    props.requiredAttributes !== undefined &&
    (!Number.isInteger(props.requiredAttributes) ||
      (props.requiredAttributes as number) < 0 ||
      (props.requiredAttributes as number) > 15)
  )
    throw new Error(
      "Shader requiredAttributes must be a stream mask between 0 and 15",
    );
  return props as unknown as ShaderProps;
}

/** Pure committed-tree assembly; upload and identity assignment belong to commits. */
export function describeShader(
  stages: readonly { type: ShaderHostType; props: ShaderProps }[],
  parameters: Readonly<Record<string, ShaderParameterKind>>,
  recipe: ShaderRecipe,
): ShaderDefinition {
  const definition: ShaderDefinition = {
    parameters: { ...parameters },
    recipe,
    backends: Object.create(null),
    requiredAttributes: 0,
  };
  for (const { type, props } of stages) {
    const backend = props.backend ?? "glsl-es-300";
    const stage = type === VERTEX_SHADER_HOST_TYPE ? "vertex" : "fragment";
    const implementation = (definition.backends[backend] ??= {});
    if (implementation[stage] !== undefined)
      throw new Error(`Duplicate ${stage} shader for ${backend}`);
    implementation[stage] = props.children;
    definition.requiredAttributes! |= props.requiredAttributes ?? 0;
    const references =
      typeof props.references === "string"
        ? props.references.trim()
          ? props.references.split(",").map((name) => name.trim())
          : []
        : (props.references ?? []);
    for (const name of references) {
      if (!/^[A-Za-z_][A-Za-z_0-9]*$/.test(name))
        throw new Error(`Invalid shader reference: ${name}`);
      if (!Object.hasOwn(parameters, name))
        throw new Error(
          `Shader reference ${name} requires a declared parameter type`,
        );
    }
  }
  return definition;
}
