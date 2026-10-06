import type { FieldValue, FieldWrite } from "@ipp/client";
import {
  byteSignature,
  isAssetReference,
  type AssetFieldWrite,
} from "../assets/declarations.js";
import { attachmentIdentity } from "../composition/attachment-identity.js";

export type DeclarationFieldValue = Exclude<
  FieldValue,
  { kind: "world" | "output" }
>;

export function fieldIdentity(value: DeclarationFieldValue): string {
  return value.kind === "bytes" || value.kind === "rows"
    ? `${value.kind}:${byteSignature(value.value)}`
    : attachmentIdentity(value);
}

export function declarationFields(
  value: unknown,
): readonly (FieldWrite | AssetFieldWrite)[] {
  if (value === undefined) return [];
  if (!Array.isArray(value))
    throw new Error("Component fields must be generated FieldWrites");
  const offsets = new Set<number>();
  for (const write of value) {
    if (
      !write ||
      !Number.isInteger(write.offset) ||
      write.offset < 0 ||
      write.offset > 0xffffffff ||
      offsets.has(write.offset)
    )
      throw new Error("Component fields require unique u32 offsets");
    offsets.add(write.offset);
    if ("asset" in write) {
      if (
        "value" in write ||
        !isAssetReference(write.asset) ||
        !write.asset.assetId
      )
        throw new Error("Invalid asset FieldWrite");
      continue;
    }
    if (
      !write.value ||
      ![
        "f32",
        "u32",
        "u64",
        "string",
        "bytes",
        "bool",
        "entity",
        "rows",
        "dynamic",
        "unset",
      ].includes(write.value.kind)
    )
      throw new Error("Unsupported declaration FieldWrite");
    if (write.value.kind === "entity" && write.value.value?.kind !== "handle")
      throw new Error("Declaration FieldWrites cannot refer to batch aliases");
  }
  return value;
}
