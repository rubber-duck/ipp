import type { Plot3dContract } from "../charts3d/content.js";
import type { ChartSpec } from "./catalog.js";

/** Dataset expressions for the gallery's animated chart parameters. */
export function chartColumnDefinitions(
  contract: Pick<Plot3dContract, "ExpressionBuilder">,
  spec: Pick<ChartSpec, "component">,
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
    if (["y", "y2", "height", "radius"].includes(column))
      value = builder.binary(
        "multiply",
        value,
        builder.fallback(
          builder.input("parameter", "f32"),
          builder.constant({ kind: "f32", value: 1 }),
        ),
      );
    if (column === "value" && spec.component.includes("Pie")) {
      const phase = builder.fallback(
        builder.input("parameter", "f32"),
        builder.constant({ kind: "f32", value: 0 }),
      );
      value = builder.binary(
        "multiply",
        value,
        builder.binary(
          "add",
          builder.constant({ kind: "f32", value: 1 }),
          builder.binary(
            "multiply",
            phase,
            builder.binary(
              "divide",
              input,
              builder.constant({ kind: "f32", value: 25 }),
            ),
          ),
        ),
      );
    }
    definitions[column] = builder.encode(value);
  }
  return definitions;
}
