/** Pure immutable expression declaration; names are resolved by the consuming System. */
export interface ExpressionDeclaration {
  inputs: readonly { name: string; kind: DatasetValueKind }[];
  nodes: readonly ExpressionNode[];
  output: number;
}
export type ExpressionUnaryOperator = keyof typeof DATA_AUTHORING.unary;
export type ExpressionBinaryOperator = keyof typeof DATA_AUTHORING.binary;
export type ExpressionNumericKind =
  | "f32"
  | "i32"
  | "u32"
  | "vec2"
  | "vec3"
  | "vec4";
export type ExpressionSignedKind = Exclude<ExpressionNumericKind, "u32">;
export type ExpressionArithmeticOperator = Exclude<
  ExpressionBinaryOperator,
  "equal" | "less" | "greater" | "and" | "or"
>;
export type ExpressionNode =
  | { operation: "input"; slot: number }
  | { operation: "constant"; value: DatasetValue }
  | { operation: "unary"; operator: ExpressionUnaryOperator; operand: number }
  | {
      operation: "binary";
      operator: ExpressionBinaryOperator;
      left: number;
      right: number;
    }
  | { operation: "clamp"; value: number; minimum: number; maximum: number }
  | { operation: "ternary"; condition: number; then: number; else: number }
  | { operation: "fallback"; value: number; replacement: number };

function expressionValue(w: Writer, value: DatasetValue): void {
  w.u8(
    WIRE[`DATASET_VALUE_${value.kind.toUpperCase()}` as keyof typeof WIRE] ??
      fail("expression kind"),
  );
  switch (value.kind) {
    case "f32":
      w.f32(value.value);
      break;
    case "i32":
      if (
        !Number.isInteger(value.value) ||
        value.value < -2147483648 ||
        value.value > 2147483647
      )
        fail("expression i32");
      w.u32(value.value >>> 0);
      break;
    case "u32":
      w.u32(value.value);
      break;
    case "bool":
      if (typeof value.value !== "boolean") fail("expression bool");
      w.u32(value.value ? 1 : 0);
      break;
    case "text":
      w.string(value.value, DATA_AUTHORING.expressionMaxStringBytes);
      break;
    default: {
      const size = { vec2: 2, vec3: 3, vec4: 4, mat2: 4, mat3: 9, mat4: 16 }[
        value.kind
      ];
      if (value.value.length !== size) fail("expression lane count");
      for (const lane of value.value) w.f32(lane);
    }
  }
}

/** Encode canonical IPPE from the executed Rust codec metadata. Core validates graph semantics on load. */
export function encodeExpression(
  declaration: ExpressionDeclaration,
): Uint8Array<ArrayBuffer> {
  const w = new Writer(DATA_AUTHORING.expressionMaxBytes);
  w.raw(Uint8Array.from(DATA_AUTHORING.expressionMagic));
  w.u32(DATA_AUTHORING.expressionVersion);
  w.count(declaration.inputs.length, DATA_AUTHORING.expressionMaxItems);
  w.count(declaration.nodes.length, DATA_AUTHORING.expressionMaxItems);
  w.u32(uint(declaration.output, declaration.nodes.length - 1));
  const names = new Set<string>();
  for (const input of declaration.inputs) {
    if (!input.name || names.has(input.name)) fail("expression input name");
    names.add(input.name);
    w.string(input.name, DATA_AUTHORING.expressionMaxStringBytes);
    w.u8(
      WIRE[`DATASET_VALUE_${input.kind.toUpperCase()}` as keyof typeof WIRE] ??
        fail("expression input type"),
    );
  }
  const node = (index: number) =>
    w.u32(uint(index, declaration.nodes.length - 1));
  for (const value of declaration.nodes) {
    w.u8(DATA_AUTHORING.nodes[value.operation]);
    switch (value.operation) {
      case "input":
        w.u32(uint(value.slot, declaration.inputs.length - 1));
        break;
      case "constant":
        expressionValue(w, value.value);
        break;
      case "unary":
        w.u8(DATA_AUTHORING.unary[value.operator]);
        node(value.operand);
        break;
      case "binary":
        w.u8(DATA_AUTHORING.binary[value.operator]);
        node(value.left);
        node(value.right);
        break;
      case "clamp":
        node(value.value);
        node(value.minimum);
        node(value.maximum);
        break;
      case "ternary":
        node(value.condition);
        node(value.then);
        node(value.else);
        break;
      case "fallback":
        node(value.value);
        node(value.replacement);
        break;
    }
  }
  return w.finish();
}

/** Builder-owned typed graph reference. Inputs may be missing at evaluation; fallback is explicit. */
export interface ExpressionRef<K extends DatasetValueKind = DatasetValueKind> {
  readonly owner: object;
  readonly index: number;
  readonly kind: K;
}
export class ExpressionBuilder {
  private readonly inputs: { name: string; kind: DatasetValueKind }[] = [];
  private readonly nodes: ExpressionNode[] = [];
  private ref<K extends DatasetValueKind>(
    kind: K,
    node: ExpressionNode,
  ): ExpressionRef<K> {
    const index = this.nodes.push(node) - 1;
    return Object.freeze({ owner: this, index, kind });
  }
  private index(value: ExpressionRef): number {
    if (value.owner !== this)
      fail("expression reference belongs to another builder");
    return value.index;
  }
  input<K extends DatasetValueKind>(name: string, kind: K): ExpressionRef<K> {
    if (!name || this.inputs.some((input) => input.name === name))
      fail("expression input name");
    const slot = this.inputs.push({ name, kind }) - 1;
    return this.ref(kind, { operation: "input", slot });
  }
  constant<K extends DatasetValueKind>(
    value: DatasetValue & { kind: K },
  ): ExpressionRef<K> {
    return this.ref(value.kind, { operation: "constant", value });
  }
  unary(
    operator: "length",
    operand: ExpressionRef<"text">,
  ): ExpressionRef<"u32">;
  unary(operator: "not", operand: ExpressionRef<"bool">): ExpressionRef<"bool">;
  unary<K extends ExpressionSignedKind>(
    operator: "negate" | "absolute",
    operand: ExpressionRef<K>,
  ): ExpressionRef<K>;
  unary(
    operator: ExpressionUnaryOperator,
    operand: ExpressionRef,
  ): ExpressionRef {
    if (
      (operator === "length" && operand.kind !== "text") ||
      (operator === "not" && operand.kind !== "bool") ||
      (["negate", "absolute"].includes(operator) &&
        !["f32", "i32", "vec2", "vec3", "vec4"].includes(operand.kind))
    )
      fail("expression unary type");
    const kind = operator === "length" ? "u32" : operand.kind;
    return this.ref(kind, {
      operation: "unary",
      operator,
      operand: this.index(operand),
    });
  }
  binary<K extends ExpressionNumericKind>(
    operator: ExpressionArithmeticOperator,
    left: ExpressionRef<K>,
    right: ExpressionRef<NoInfer<K>>,
  ): ExpressionRef<K>;
  binary<K extends DatasetValueKind>(
    operator: "equal",
    left: ExpressionRef<K>,
    right: ExpressionRef<NoInfer<K>>,
  ): ExpressionRef<"bool">;
  binary<K extends "f32" | "i32" | "u32">(
    operator: "less" | "greater",
    left: ExpressionRef<K>,
    right: ExpressionRef<NoInfer<K>>,
  ): ExpressionRef<"bool">;
  binary(
    operator: "and" | "or",
    left: ExpressionRef<"bool">,
    right: ExpressionRef<"bool">,
  ): ExpressionRef<"bool">;
  binary(
    operator: ExpressionBinaryOperator,
    left: ExpressionRef,
    right: ExpressionRef,
  ): ExpressionRef {
    if (left.kind !== right.kind) fail("expression operand kinds differ");
    const allowed =
      operator === "equal" ||
      (["less", "greater"].includes(operator)
        ? ["f32", "i32", "u32"].includes(left.kind)
        : ["and", "or"].includes(operator)
          ? left.kind === "bool"
          : ["f32", "i32", "u32", "vec2", "vec3", "vec4"].includes(left.kind));
    if (!allowed) fail("expression binary type");
    const kind = ["equal", "less", "greater"].includes(operator)
      ? "bool"
      : left.kind;
    return this.ref(kind, {
      operation: "binary",
      operator,
      left: this.index(left),
      right: this.index(right),
    });
  }
  fallback<K extends DatasetValueKind>(
    value: ExpressionRef<K>,
    replacement: ExpressionRef<NoInfer<K>>,
  ): ExpressionRef<K> {
    if (value.kind !== replacement.kind)
      fail("expression fallback kinds differ");
    return this.ref(value.kind, {
      operation: "fallback",
      value: this.index(value),
      replacement: this.index(replacement),
    });
  }
  clamp<K extends ExpressionNumericKind>(
    value: ExpressionRef<K>,
    minimum: ExpressionRef<NoInfer<K>>,
    maximum: ExpressionRef<NoInfer<K>>,
  ): ExpressionRef<K> {
    if (value.kind !== minimum.kind || value.kind !== maximum.kind)
      fail("expression clamp kinds differ");
    return this.ref(value.kind, {
      operation: "clamp",
      value: this.index(value),
      minimum: this.index(minimum),
      maximum: this.index(maximum),
    });
  }
  select<K extends DatasetValueKind>(
    condition: ExpressionRef<"bool">,
    then: ExpressionRef<K>,
    otherwise: ExpressionRef<NoInfer<K>>,
  ): ExpressionRef<K> {
    if (condition.kind !== "bool" || then.kind !== otherwise.kind)
      fail("expression branch kinds differ");
    return this.ref(then.kind, {
      operation: "ternary",
      condition: this.index(condition),
      then: this.index(then),
      else: this.index(otherwise),
    });
  }
  declaration(output: ExpressionRef): ExpressionDeclaration {
    return {
      inputs: this.inputs.map((input) => ({ ...input })),
      nodes: [...this.nodes],
      output: this.index(output),
    };
  }
  encode(output: ExpressionRef): Uint8Array<ArrayBuffer> {
    return encodeExpression(this.declaration(output));
  }
}

function authoringName(name: string): Uint8Array<ArrayBuffer> {
  const w = new Writer(DATA_AUTHORING.expressionMaxBytes);
  // Reuse the generated UTF-16 validation and UTF-8 encoder; strip its u32 length.
  w.string(name, DATA_AUTHORING.expressionMaxStringBytes);
  return w.finish().subarray(4);
}

export type DataWindow =
  | { kind: "count"; count: bigint }
  | {
      kind: "range";
      column: string;
      width: number;
      anchor:
        | { kind: "latest" }
        | { kind: "hostTime"; unitsPerSecond: number }
        | {
            kind: "supplied";
            value: { kind: "f32" | "i32" | "u32"; value: number };
          };
    };

/** Empty bytes mean no constraints. Schema/window admissibility is validated by DataService. */
export function encodeDataWindows(
  windows: readonly DataWindow[],
): Uint8Array<ArrayBuffer> {
  if (!windows.length) return new Uint8Array();
  const w = new Writer(FIELD_BYTES);
  w.raw(Uint8Array.from(DATA_AUTHORING.windowHeader));
  w.u16(windows.length);
  for (const window of windows) {
    if (window.kind === "count") {
      if (window.count > (1n << BigInt(TARGET.pointerBits)) - 1n)
        fail("window count exceeds target usize");
      w.u8(DATA_AUTHORING.windowCount);
      w.u64(window.count);
    } else {
      if (!window.column || !Number.isFinite(window.width) || window.width < 0)
        fail("range window");
      w.u8(DATA_AUTHORING.windowRange);
      const name = authoringName(window.column);
      w.u16(name.length);
      w.raw(name);
      w.f64(window.width);
      switch (window.anchor.kind) {
        case "latest":
          w.u8(DATA_AUTHORING.anchorLatest);
          break;
        case "hostTime":
          if (window.anchor.unitsPerSecond <= 0) fail("window Host time units");
          w.u8(DATA_AUTHORING.anchorHostTime);
          w.f64(window.anchor.unitsPerSecond);
          break;
        case "supplied":
          w.u8(DATA_AUTHORING.anchorSupplied);
          expressionValue(w, window.anchor.value);
          break;
      }
    }
  }
  return w.finish();
}

export interface ExpressionDriverInput {
  name: string;
  property: { component: number; offset: number };
}
/** Encode canonical name-sorted IPDI. Use generated offsets or observed dynamic descriptors. */
export function encodeExpressionDriverInputs(
  inputs: readonly ExpressionDriverInput[],
): Uint8Array<ArrayBuffer> {
  if (!inputs.length) return new Uint8Array();
  if (inputs.length > DATA_AUTHORING.driverMaxInputs)
    fail("expression driver input count");
  const w = new Writer(MAX_MESSAGE_BYTES);
  w.raw(Uint8Array.from(DATA_AUTHORING.driverHeader));
  w.u16(inputs.length);
  // Rust orders UTF-8 bytes; JS UTF-16 ordering differs for astral Unicode names.
  const encoded = inputs.map((input) => ({
    input,
    name: authoringName(input.name),
  }));
  encoded.sort((a, b) => {
    for (let i = 0; i < Math.min(a.name.length, b.name.length); i++) {
      const difference = a.name[i]! - b.name[i]!;
      if (difference) return difference;
    }
    return a.name.length - b.name.length;
  });
  let previous: string | undefined;
  for (const { input, name } of encoded) {
    if (!name.length || input.name === previous)
      fail("expression driver input name");
    previous = input.name;
    w.u16(name.length);
    w.raw(name);
    w.u16(input.property.component);
    w.u32(input.property.offset);
  }
  return w.finish();
}
