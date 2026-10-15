import { createElement } from "react";
import { Asset } from "./declarations.js";
import { useAssetId } from "../scope/identity.js";
import { useOptionalWorld, type ReactAssetCodec } from "../scope/world.js";

export interface MeshAssetProps<T> {
  id?: string;
  data: T;
  variant?: number;
  encode?: (data: T) => Uint8Array<ArrayBuffer>;
}

/** Immutable mesh data uses the receiving session's explicitly installed mesh codec. */
export function MeshAsset<T>({ id, data, variant, encode }: MeshAssetProps<T>) {
  const assetId = useAssetId(id);
  const scope = useOptionalWorld();
  // Applications install their mesh input codec explicitly; the service catalog
  // erases its input type while this declaration retains the author's T.
  const codec = scope?.codecs?.mesh as ReactAssetCodec<T> | undefined;
  if (codec && codec.kind !== 1)
    throw new Error("MeshAsset requires a mesh resource codec (kind 1)");
  const encoder = encode ?? codec?.encode;
  if (!encoder)
    throw new Error(
      "MeshAsset needs a mesh codec in createRoot options or an explicit encode prop",
    );
  return createElement(Asset<T>, {
    id: assetId,
    kind: codec?.kind ?? 1,
    data,
    encode: encoder,
    ...(variant === undefined ? {} : { variant }),
  });
}

export interface FontAssetProps {
  id?: string;
  /** An immutable IPPF font product, already encoded independently of component layouts. */
  data: Uint8Array<ArrayBuffer>;
}

const bytes = (data: Uint8Array<ArrayBuffer>) => data;

export function FontAsset({ id, data }: FontAssetProps) {
  const assetId = useAssetId(id);
  return createElement(Asset<Uint8Array<ArrayBuffer>>, {
    id: assetId,
    kind: 17,
    data,
    encode: bytes,
  });
}
