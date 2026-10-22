/** Base and scalar contribution use one shared, pixel-preserving tile mapping. */
import { textureSource, type CockpitLightTile } from "./runtime.js";

export function lightTextureParameters(count: number) {
  return Object.fromEntries(
    Array.from(
      { length: count },
      (_, index) =>
        [
          [`base${index}`, "texture2D"],
          [`contribution${index}`, "texture2D"],
          [`region${index}`, "vec4"],
          [`textureRect${index}`, "vec4"],
        ] as const,
    ).flat(),
  );
}

export function lightTextureProperties(tiles: readonly CockpitLightTile[]) {
  const texture = (source: string) =>
    ({
      kind: "asset",
      value: { kind: 2, source: textureSource(source) },
    }) as const;
  return Object.fromEntries(
    tiles.flatMap((tile, index) => [
      [`base${index}`, texture(tile.baseTexture)],
      [`contribution${index}`, texture(tile.contributionTexture)],
      [`region${index}`, tile.bounds],
      [`textureRect${index}`, tile.textureRect],
    ]),
  );
}

export function lightTextureSampling(count: number): string {
  return ["base", "contribution"]
    .map(
      (role) => `
vec4 cockpit${role === "base" ? "Base" : "Contribution"}(vec2 uv) {
  ${Array.from(
    { length: count },
    (_, index) => `
  ${index < count - 1 ? `if (uv.x < p_region${index}.z)` : ""} {
    vec2 local = (uv - p_region${index}.xy) / (p_region${index}.zw - p_region${index}.xy);
    vec2 mapped = mix(p_textureRect${index}.xy, p_textureRect${index}.zw, local);
    return ippSample(p_${role}${index}, mapped);
  }`,
  ).join("\n")}
}`,
    )
    .join("\n");
}
