import type { Plot3dContract } from "../charts3d/content.js";

/** Source values pass through unchanged; invalid samples remain gaps. */
export function chartColumnDefinitions(
  contract: Pick<Plot3dContract, "ExpressionBuilder">,
): Record<string, Uint8Array<ArrayBuffer>> {
  const definitions: Record<string, Uint8Array<ArrayBuffer>> = {};
  for (const column of [
    "x",
    "y",
    "z",
    "y2",
    "value",
    "radius",
    "height",
    "color",
  ]) {
    const builder = new contract.ExpressionBuilder();
    if (column === "color") {
      definitions[column] = builder.encode(
        builder.input("column:color", "vec4"),
      );
      continue;
    }
    const input = builder.input(
      `column:${column === "value" ? "y" : column}`,
      "f32",
    );
    let value = input;
    if (column === "y" || column === "y2")
      value = builder.binary(
        "divide",
        value,
        builder.input("column:valid", "f32"),
      );
    definitions[column] = builder.encode(value);
  }
  return definitions;
}
