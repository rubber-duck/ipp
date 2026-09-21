/** Declarative immutable resources; all external effects belong to committed snapshots. */
import { createElement, type ReactNode } from "react";
import type {
  AnimationClipSource,
  ShaderParameterKind,
  ShaderRecipe,
} from "@ipp/client";

export const ANIMATION_ASSET_HOST_TYPE = "ipp-animation-asset";
export const ASSET_HOST_TYPE = "ipp-asset";
export const SHADER_ASSET_HOST_TYPE = "ipp-shader-asset";
export type AssetHostType =
  | typeof ANIMATION_ASSET_HOST_TYPE
  | typeof ASSET_HOST_TYPE
  | typeof SHADER_ASSET_HOST_TYPE;

export interface AssetReference {
  readonly assetId: string;
}

export function assetRef(id: string): AssetReference {
  if (!id) throw new Error("Asset reference requires an id");
  return Object.freeze({ assetId: id });
}

export function isAssetReference(value: unknown): value is AssetReference {
  return (
    typeof value === "object" &&
    value !== null &&
    "assetId" in value &&
    typeof value.assetId === "string"
  );
}

export interface AssetProps<T> {
  id: string;
  kind: number;
  variant?: number;
  /** Immutable input: replace this value when content changes. */
  data: T;
  encode: (data: T) => Uint8Array<ArrayBuffer>;
  children?: ReactNode;
}

export function Asset<T>(props: AssetProps<T>) {
  return createElement(ASSET_HOST_TYPE, props);
}

export interface AnimationAssetProps {
  id: string;
  clip: AnimationClipSource;
}

export function AnimationAsset({ id, clip }: AnimationAssetProps) {
  return createElement(ANIMATION_ASSET_HOST_TYPE, { id, clip });
}

export interface ShaderAssetProps {
  id: string;
  recipe: ShaderRecipe;
  parameters: Readonly<Record<string, ShaderParameterKind>>;
  children?: ReactNode;
}

export function ShaderAsset(props: ShaderAssetProps) {
  return createElement(SHADER_ASSET_HOST_TYPE, props);
}

export function isAsset(type: string): type is AssetHostType {
  return (
    type === ANIMATION_ASSET_HOST_TYPE ||
    type === ASSET_HOST_TYPE ||
    type === SHADER_ASSET_HOST_TYPE
  );
}

export interface AssetDescription {
  readonly identity: number;
  readonly id: string;
  readonly kind: number;
  readonly variant: number;
  readonly bytes: Uint8Array<ArrayBuffer>;
  readonly signature: string;
  readonly version: number;
}

/** Exact content signature: equality never depends on a hash collision. */
export function byteSignature(bytes: Uint8Array): string {
  let result = "";
  for (let offset = 0; offset < bytes.length; offset += 8192)
    result += String.fromCharCode(...bytes.subarray(offset, offset + 8192));
  return result;
}
