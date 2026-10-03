import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const directory = resolve(process.argv[2]);
const expected = JSON.parse(
  await readFile(resolve(directory, "data-authoring.json"), "utf8"),
);
for (const target of ["native", "wasm"]) {
  const module = resolve(directory, `${target}-authoring.mjs`);
  await build({
    entryPoints: [resolve(directory, `${target}.ts`)],
    outfile: module,
    bundle: true,
    platform: "node",
    format: "esm",
    logLevel: "warning",
  });
  const codec = await import(pathToFileURL(module));
  const builder = new codec.ExpressionBuilder();
  const x = builder.input("x", "f32"),
    parameter = builder.input("parameter", "f32");
  const two = builder.constant({ kind: "f32", value: 2 });
  const fallback = builder.fallback(parameter, two),
    scaled = builder.binary("multiply", x, fallback);
  const zero = builder.constant({ kind: "f32", value: 0 }),
    ten = builder.constant({ kind: "f32", value: 10 });
  const clamped = builder.clamp(scaled, zero, ten),
    truth = builder.constant({ kind: "bool", value: true });
  const selected = builder.select(truth, clamped, zero),
    text = builder.constant({ kind: "text", value: "α🙂" });
  const length = builder.unary("length", text),
    unsigned = builder.constant({ kind: "u32", value: 0xffffffff });
  builder.binary("equal", length, unsigned);
  builder.constant({ kind: "mat2", value: [1, 2, 3, 4] });
  const negated = builder.unary("negate", x);
  builder.unary("absolute", negated);
  builder.unary("not", truth);
  builder.constant({ kind: "i32", value: -2147483648 });
  builder.constant({ kind: "vec2", value: [-0, 2] });
  assert.deepEqual(
    [...builder.encode(selected)],
    expected.expression,
    `${target}: canonical core IPPE`,
  );
  assert.deepEqual(
    [...codec.encodeExpression(builder.declaration(selected))],
    expected.expression,
  );
  assert.deepEqual(
    [
      ...codec.encodeDataWindows([
        { kind: "count", count: 3n },
        {
          kind: "range",
          column: "raw α",
          width: 2,
          anchor: { kind: "latest" },
        },
        {
          kind: "range",
          column: "raw α",
          width: 1,
          anchor: { kind: "hostTime", unitsPerSecond: 1000 },
        },
        {
          kind: "range",
          column: "raw α",
          width: 2,
          anchor: { kind: "supplied", value: { kind: "i32", value: -3 } },
        },
      ]),
    ],
    expected.windows,
    `${target}: canonical core IPPW`,
  );
  assert.deepEqual(
    [
      ...codec.encodeExpressionDriverInputs([
        { name: "𐀀", property: { component: 100, offset: 1234 } },
        { name: "\ue000", property: { component: 101, offset: 65536 } },
      ]),
    ],
    expected.drivers,
    `${target}: canonical core IPDI UTF-8 order`,
  );
  assert.equal(codec.encodeDataWindows([]).length, 0);
  assert.equal(codec.encodeExpressionDriverInputs([]).length, 0);
  assert.throws(() =>
    builder.encode(
      new codec.ExpressionBuilder().constant({ kind: "f32", value: 1 }),
    ),
  );
  assert.throws(() => builder.binary("add", truth, truth));
  assert.throws(() => builder.unary("length", x));
  assert.throws(() =>
    codec.encodeExpressionDriverInputs([
      { name: "\ud800", property: { component: 1, offset: 0 } },
    ]),
  );
  if (codec.TARGET.pointerBits === 32)
    assert.throws(() =>
      codec.encodeDataWindows([{ kind: "count", count: 1n << 32n }]),
    );
  else
    assert.ok(
      codec.encodeDataWindows([{ kind: "count", count: 1n << 32n }]).length > 0,
    );
  await writeFile(
    resolve(directory, `${target}-authoring-types.ts`),
    `
import {ExpressionBuilder} from './${target}.js';
const b = new ExpressionBuilder();
const scalar = b.input('scalar', 'f32'), integer = b.input('int', 'i32'), text = b.input('text', 'text');
b.binary('add', scalar, scalar);
b.unary('length', text);
// @ts-expect-error Operand kinds must match exactly.
b.binary('add', scalar, integer);
// @ts-expect-error Text supports explicit selection rather than arithmetic.
b.binary('multiply', text, text);
// @ts-expect-error Length requires text.
b.unary('length', scalar);
// @ts-expect-error Fallback kinds must match exactly.
b.fallback(scalar, integer);
`,
  );
}
console.log(
  "Verified native/WASM authoring bytes against canonical core encoders and target usize bounds.",
);
