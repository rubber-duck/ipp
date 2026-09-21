/** Named typed component parameters; CPU descriptors and GPU packing are independent. */
import type { Command, EntityRef, StateOverlayRef } from "./types.js";

export type DynamicPropertyKind =
  | "f32"
  | "i32"
  | "u32"
  | "bool"
  | "vec2"
  | "vec3"
  | "vec4"
  | "mat2"
  | "mat3"
  | "mat4"
  | "asset";
export type DynamicValue =
  | { kind: "f32" | "i32" | "u32"; value: number }
  | { kind: "bool"; value: boolean }
  | { kind: "vec2"; value: readonly [number, number] }
  | { kind: "vec3"; value: readonly [number, number, number] }
  | { kind: "vec4" | "mat2"; value: readonly [number, number, number, number] }
  | { kind: "mat3" | "mat4"; value: readonly number[] }
  | {
      kind: "asset";
      value: { kind: number; source: string; variant?: number };
    };

/** Natural authoring values; numbers default to f32 and strings select textures. */
export type DynamicPropertyInput =
  | DynamicValue
  | number
  | boolean
  | string
  | readonly number[];

type Pair = readonly [number, number];
type Triple = readonly [number, number, number];
type Quad = readonly [number, number, number, number];
type Matrix3 = readonly [
  number,
  number,
  number,
  number,
  number,
  number,
  number,
  number,
  number,
];
type Matrix4 = readonly [
  number,
  number,
  number,
  number,
  number,
  number,
  number,
  number,
  number,
  number,
  number,
  number,
  number,
  number,
  number,
  number,
];
type TypedValue<K extends DynamicPropertyKind> = DynamicValue & { kind: K };

function typed<K extends DynamicPropertyKind>(
  kind: K,
  value: unknown,
): TypedValue<K> {
  const result = {
    kind,
    value: Array.isArray(value) ? [...value] : value,
  } as TypedValue<K>;
  encodeDynamicValue(result);
  return result;
}

function vectorArguments(values: readonly unknown[]): unknown {
  return values.length === 1 && Array.isArray(values[0]) ? values[0] : values;
}

export const f32 = (value: number): TypedValue<"f32"> => typed("f32", value);
export const i32 = (value: number): TypedValue<"i32"> => typed("i32", value);
export const u32 = (value: number): TypedValue<"u32"> => typed("u32", value);
export const bool = (value: boolean): TypedValue<"bool"> =>
  typed("bool", value);
export const vec2 = (...value: Pair | [Pair]): TypedValue<"vec2"> =>
  typed("vec2", vectorArguments(value));
export const vec3 = (...value: Triple | [Triple]): TypedValue<"vec3"> =>
  typed("vec3", vectorArguments(value));
export const vec4 = (...value: Quad | [Quad]): TypedValue<"vec4"> =>
  typed("vec4", vectorArguments(value));
/** Column-major matrices; four untagged numbers otherwise infer vec4. */
export const mat2 = (...value: Quad | [Quad]): TypedValue<"mat2"> =>
  typed("mat2", vectorArguments(value));
export const mat3 = (...value: Matrix3 | [Matrix3]): TypedValue<"mat3"> =>
  typed("mat3", vectorArguments(value));
export const mat4 = (...value: Matrix4 | [Matrix4]): TypedValue<"mat4"> =>
  typed("mat4", vectorArguments(value));
/** Bind an asset of any compiled payload type; the consumer validates its requirements. */
export const asset = (
  kind: number,
  source: string,
  variant = 0,
): TypedValue<"asset"> => typed("asset", { kind, source, variant });

/** Convenience for an ordinary asset reference with the compiled texture type (2). */
export const texture2D = (source: string, variant = 0): TypedValue<"asset"> =>
  asset(2, source, variant);

/** Infer and detach a canonical typed value for a committed declaration. */
export function inferDynamicValue(value: unknown): DynamicValue {
  let result: DynamicValue;
  if (typeof value === "number") result = f32(value);
  else if (typeof value === "boolean") result = bool(value);
  else if (typeof value === "string") result = texture2D(value);
  else if (Array.isArray(value)) {
    const kind = (
      { 2: "vec2", 3: "vec3", 4: "vec4", 9: "mat3", 16: "mat4" } as const
    )[value.length as 2 | 3 | 4 | 9 | 16];
    if (!kind) throw new Error("Expected 2, 3, 4, 9, or 16 numeric lanes");
    result = typed(kind, value);
  } else if (
    value &&
    typeof value === "object" &&
    Object.hasOwn(value, "kind")
  ) {
    result = value as DynamicValue;
  } else
    throw new Error(
      "Expected a number, boolean, numeric vector/matrix, texture source, or typed value helper",
    );
  return decodeDynamicValue(encodeDynamicValue(result));
}

const kinds: readonly DynamicPropertyKind[] = [
  "f32",
  "i32",
  "u32",
  "bool",
  "vec2",
  "vec3",
  "vec4",
  "mat2",
  "mat3",
  "mat4",
  "asset",
];
const lanes = [1, 1, 1, 1, 2, 3, 4, 4, 9, 16, 0];
const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });

function uint(value: number): number {
  if (!Number.isInteger(value) || value < 0 || value > 0xffff_ffff)
    throw new Error("Expected u32");
  return value;
}

function assetType(value: number): number {
  if (uint(value) > 0xffff) throw new Error("Expected asset type u16");
  return value;
}

function dynamicKind(tag: number): DynamicPropertyKind | undefined {
  // Tag 11 was the retired texture-only value; never reinterpret its payload.
  return tag === 12
    ? "asset"
    : tag >= 1 && tag <= 10
      ? kinds[tag - 1]
      : undefined;
}

export function encodeDynamicValue(
  value: DynamicValue,
): Uint8Array<ArrayBuffer> {
  const index = kinds.indexOf(value.kind);
  if (index < 0) throw new Error("Unknown dynamic property kind");
  if (value.kind === "asset") {
    if (typeof value.value.source !== "string")
      throw new Error("Expected asset source string");
    const source = encoder.encode(value.value.source);
    if (decoder.decode(source) !== value.value.source)
      throw new Error("Invalid asset source");
    const bytes = new Uint8Array(7 + source.length);
    bytes[0] = 12;
    const view = new DataView(bytes.buffer);
    view.setUint16(1, assetType(value.value.kind), true);
    view.setUint32(3, uint(value.value.variant ?? 0), true);
    bytes.set(source, 7);
    return bytes;
  }
  const values = Array.isArray(value.value) ? value.value : [value.value];
  if (values.length !== lanes[index])
    throw new Error("Dynamic property lane count mismatch");
  const bytes = new Uint8Array(1 + values.length * 4),
    view = new DataView(bytes.buffer);
  bytes[0] = index + 1;
  values.forEach((item, i) => {
    if (value.kind === "bool") {
      if (typeof item !== "boolean") throw new Error("Expected boolean");
      view.setUint32(1 + i * 4, Number(item), true);
    } else if (value.kind === "u32")
      view.setUint32(1 + i * 4, uint(item as number), true);
    else if (value.kind === "i32") {
      if (
        !Number.isInteger(item) ||
        (item as number) < -2147483648 ||
        (item as number) > 2147483647
      )
        throw new Error("Expected i32");
      view.setInt32(1 + i * 4, item as number, true);
    } else {
      if (typeof item !== "number" || !Number.isFinite(Math.fround(item)))
        throw new Error("Expected finite f32");
      view.setFloat32(1 + i * 4, item, true);
    }
  });
  return bytes;
}

export function decodeDynamicValue(bytes: Uint8Array): DynamicValue {
  const kind = dynamicKind(bytes[0]!),
    index = kind ? kinds.indexOf(kind) : -1;
  if (!kind) throw new Error("Unknown dynamic property kind");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (kind === "asset") {
    if (bytes.length < 7) throw new Error("Truncated asset property");
    return {
      kind,
      value: {
        kind: view.getUint16(1, true),
        variant: view.getUint32(3, true),
        source: decoder.decode(bytes.subarray(7)),
      },
    };
  }
  if (bytes.length !== 1 + lanes[index]! * 4)
    throw new Error("Dynamic property length mismatch");
  if (kind === "bool") {
    const value = view.getUint32(1, true);
    if (value > 1) throw new Error("Invalid dynamic boolean");
    return { kind, value: value === 1 };
  }
  if (kind === "i32") return { kind, value: view.getInt32(1, true) };
  if (kind === "u32") return { kind, value: view.getUint32(1, true) };
  const values = Array.from({ length: lanes[index]! }, (_, i) =>
    view.getFloat32(1 + i * 4, true),
  );
  if (values.some((v) => !Number.isFinite(v)))
    throw new Error("Nonfinite dynamic value");
  return (
    kind === "f32" ? { kind, value: values[0]! } : { kind, value: values }
  ) as DynamicValue;
}

/** Decode core inspection metadata; keys are opaque lifetime identities, not GPU offsets. */
export function decodeDynamicDescriptors(
  bytes: Uint8Array,
): Map<number, { name: string; kind: DynamicPropertyKind }> {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let at = 0;
  const u32 = () => {
    const value = view.getUint32(at, true);
    at += 4;
    return value;
  };
  const next = u32(),
    count = u32(),
    result = new Map<number, { name: string; kind: DynamicPropertyKind }>(),
    names = new Set<string>();
  if (next <= 0x80000000 || next === 0xffffffff || count > bytes.length / 10)
    throw new Error("Invalid dynamic descriptors");
  for (let i = 0; i < count; i++) {
    const length = u32();
    if (length > bytes.length - at) throw new Error("Truncated property name");
    const name = decoder.decode(bytes.subarray(at, at + length));
    at += length;
    const key = u32(),
      kind = dynamicKind(bytes[at++]!);
    if (
      !kind ||
      !/^[A-Za-z_][A-Za-z_0-9]*$/.test(name) ||
      key <= 0x80000000 ||
      key >= next ||
      result.has(key) ||
      names.has(name)
    )
      throw new Error("Invalid dynamic descriptor");
    names.add(name);
    result.set(key, { name, kind });
  }
  if (at !== bytes.length) throw new Error("Trailing dynamic descriptors");
  return result;
}

export const DynamicProperty = {
  set(
    entity: EntityRef,
    component: number,
    name: string,
    value: DynamicValue,
  ): Command {
    return { kind: "setDynamicProperty", entity, component, name, value };
  },
  remove(entity: EntityRef, component: number, name: string): Command {
    return { kind: "removeDynamicProperty", entity, component, name };
  },
  override(
    owner: StateOverlayRef,
    overlay: StateOverlayRef,
    properties: Record<string, DynamicValue>,
    clear: string[] = [],
  ): Command {
    return {
      kind: "updateDynamicComponentStateOverlay",
      owner,
      overlay,
      properties,
      clear,
    };
  },
};

/** Shader requirements are separate from component property storage types. */
export type ShaderParameterKind =
  | Exclude<DynamicPropertyKind, "asset">
  | "texture2D";
const shaderKinds: readonly ShaderParameterKind[] = [
  "f32",
  "i32",
  "u32",
  "bool",
  "vec2",
  "vec3",
  "vec4",
  "mat2",
  "mat3",
  "mat4",
  "texture2D",
];

/** Interpret an authored value when constructing a shader requirement. */
export function shaderParameterKind(value: DynamicValue): ShaderParameterKind {
  if (value.kind !== "asset") return value.kind;
  if (value.value.kind !== 2)
    throw new Error("Shader parameter requires a 2D texture asset");
  return "texture2D";
}

export interface ShaderRecipe {
  backend?: string;
  normals?: boolean;
  skinning?: boolean;
  meshPose?: boolean;
  lighting?: boolean;
  shadowPass?: boolean;
  /** Vertex inputs include per-instance model transforms. */
  instancing?: boolean;
}

export interface ShaderDefinition {
  recipe?: ShaderRecipe;
  parameters: Record<string, ShaderParameterKind>;
  backends: Record<string, { vertex?: string; fragment?: string }>;
  /** Bit 0 color, bit 1 UV, bit 2 normal, bit 3 texture weight. */
  requiredAttributes?: number;
}

/** Encode immutable IPPH v2 source for the ordinary owned asset data plane. */
export function encodeShaderDefinition(
  definition: ShaderDefinition,
): Uint8Array<ArrayBuffer> {
  const chunks: Uint8Array[] = [];
  const u32 = (n: number) => {
    const b = new Uint8Array(4);
    new DataView(b.buffer).setUint32(0, uint(n), true);
    chunks.push(b);
  };
  const text = (s: string) => {
    const bytes = encoder.encode(s);
    u32(bytes.length);
    chunks.push(bytes);
  };
  chunks.push(new Uint8Array([73, 80, 80, 72]));
  u32(2);
  const recipe = definition.recipe ?? {};
  const flags = [
    recipe.normals,
    recipe.skinning,
    recipe.meshPose,
    recipe.lighting,
    recipe.shadowPass,
    recipe.instancing,
  ];
  if (flags.some((value) => value !== undefined && typeof value !== "boolean"))
    throw new Error("Invalid shader recipe flags");
  u32(
    flags.reduce<number>(
      (bits, value, index) => bits | (value ? 1 << index : 0),
      0,
    ),
  );
  text(recipe.backend ?? "glsl-es-300");
  u32(definition.requiredAttributes ?? 0);
  const parameters = Object.entries(definition.parameters).sort(([a], [b]) =>
    a < b ? -1 : a > b ? 1 : 0,
  );
  u32(parameters.length);
  for (const [name, kind] of parameters) {
    if (!/^[A-Za-z_][A-Za-z_0-9]*$/.test(name) || shaderKinds.indexOf(kind) < 0)
      throw new Error("Invalid shader parameter");
    text(name);
    chunks.push(new Uint8Array([shaderKinds.indexOf(kind) + 1]));
  }
  const backends = Object.entries(definition.backends).sort(([a], [b]) =>
    a < b ? -1 : a > b ? 1 : 0,
  );
  u32(backends.length);
  for (const [name, source] of backends) {
    text(name);
    text(source.vertex ?? "");
    text(source.fragment ?? "");
  }
  const bytes = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0));
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.length;
  }
  return bytes;
}
