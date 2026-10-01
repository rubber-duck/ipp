function tagged(value: unknown): unknown {
  if (value === null) return ["null"];
  if (Array.isArray(value)) return ["array", value.map(tagged)];
  switch (typeof value) {
    case "object":
      return [
        "object",
        Object.entries(value)
          .sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0))
          .map(([key, field]) => [key, tagged(field)]),
      ];
    case "number":
      return ["number", Object.is(value, -0) ? "-0" : String(value)];
    case "bigint":
      return ["bigint", String(value)];
    case "string":
    case "boolean":
      return [typeof value, value];
    case "undefined":
      return ["undefined"];
    default:
      throw new Error("Unsupported attachment identity value");
  }
}

export function attachmentIdentity(value: unknown): string {
  return JSON.stringify(tagged(value));
}
