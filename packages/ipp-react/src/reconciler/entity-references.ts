import type { ReactEntityDescription } from "./tree.js";

export type ReactEntityReference = bigint | { entity: number };

export function describeEntityReference(
  reference: string | bigint,
  entities: readonly ReactEntityDescription[],
): ReactEntityReference {
  if (typeof reference === "bigint") return reference;
  const matches = entities.filter((entity) => entity.symbolicId === reference);
  if (matches.length !== 1)
    throw new Error(`Reference must identify one scene Entity: ${reference}`);
  return { entity: matches[0]!.identity };
}
