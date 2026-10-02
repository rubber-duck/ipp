import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const directory = path.resolve(process.argv[2]);
const report = JSON.parse(
  await readFile(path.join(directory, "target-report.json"), "utf8"),
);
const kinds = {
  1: "f32",
  2: "i32",
  3: "u32",
  4: "bool",
  5: "vec2",
  6: "vec3",
  7: "vec4",
  12: "asset",
  13: "text",
};
const results = [];
for (const target of ["native", "wasm"]) {
  const source = path.join(directory, `${target}.ts`);
  const output = path.join(directory, `${target}-rows.mjs`);
  await build({
    entryPoints: [source],
    outfile: output,
    bundle: true,
    platform: "node",
    format: "esm",
    logLevel: "warning",
  });
  const codec = await import(pathToFileURL(output));
  const fixture = report[`${target}Fixture`];
  const descriptor = fixture.rows.fields.rows;
  const layout = {
    ...descriptor.rows,
    properties: descriptor.rows.properties.map((property) => ({
      ...property,
      kind: kinds[property.kind],
      hint: property.hint === 1 ? "rotation" : "none",
    })),
  };
  const component = {
    id: 60001,
    fields: { rows: { ...descriptor, rows: layout } },
  };
  const full = {
    weight: -2.5,
    rotation: [0, 0, 0, 1],
    source: { kind: 2, source: "memory:✓", variant: 7 },
    label: "é🙂",
    flag: true,
    count: 0xffffffff,
    signed: -2147483648,
    pair: [-0, 2],
    triple: [1, 2, 3],
  };
  const table = {
    nextSlot: 8,
    rows: new Map([
      [4, full],
      [1, { weight: 0.5, label: "ok" }],
    ]),
  };
  const bytes = codec.encodeRowsTable(layout, table);
  assert.deepEqual(
    [...bytes],
    fixture.rowsExample,
    `${target}: actual core encoding`,
  );
  assert.deepEqual(codec.decodeRowsTable(layout, bytes), {
    nextSlot: 8,
    rows: new Map([...table.rows].sort(([left], [right]) => left - right)),
  });
  assert.deepEqual([...table.rows.keys()], [4, 1]);
  assert.deepEqual(
    [
      ...codec.encodeRowsTable(layout, {
        nextSlot: 2,
        rows: new Map([[1, { weight: 0.5, label: "ok" }]]),
      }),
    ],
    descriptor.default,
  );
  const maxSlots = Math.floor(0x10000000 / layout.properties.length);
  assert.equal(
    codec.decodeRowsTable(
      layout,
      codec.encodeRowsTable(layout, { nextSlot: maxSlots, rows: new Map() }),
    ).nextSlot,
    maxSlots,
  );
  assert.deepEqual(
    codec.decodeRowsTable(
      layout,
      codec.encodeRowsTable(layout, { nextSlot: 0, rows: new Map() }),
    ),
    { nextSlot: 0, rows: new Map() },
  );
  for (const nextSlot of [-1, 1.5, NaN, Infinity, maxSlots + 1])
    assert.throws(() => codec.encodeRowsTable(layout, { ...table, nextSlot }));
  for (const slot of [-1, 1.5, NaN, Infinity, 8])
    assert.throws(() =>
      codec.encodeRowsTable(layout, {
        nextSlot: 8,
        rows: new Map([[slot, full]]),
      }),
    );
  for (const [property, value] of [
    ["weight", null],
    ["weight", undefined],
    ["weight", [1]],
    ["weight", NaN],
    ["weight", 1e100],
    ["flag", 1],
    ["count", -1],
    ["count", 2 ** 32],
    ["signed", 2147483648],
    ["signed", 1.5],
    ["pair", [1]],
    ["pair", new Array(2)],
    ["pair", [1, 2, 3]],
    ["triple", [1, Infinity, 3]],
    ["rotation", [1, 2, 3]],
    ["label", "🙂🙂a"],
    ["label", "\ud800"],
    ["source", { kind: 65536, source: "x" }],
    ["source", { kind: 2, source: "\ud800" }],
    ["source", { kind: 2, source: "x", variant: -1 }],
    ["absent", 1],
  ]) {
    assert.throws(
      () =>
        codec.encodeRowsTable(layout, {
          nextSlot: 1,
          rows: new Map([[0, { ...full, [property]: value }]]),
        }),
      `${property}:${String(value)}`,
    );
    if (value !== null && value !== undefined)
      assert.throws(() =>
        codec.rowPatchFields(component, "rows", 1, { [property]: value }),
      );
  }
  assert.throws(() =>
    codec.encodeRowsTable(layout, { nextSlot: 1, rows: new Map([[0, {}]]) }),
  );
  assert.throws(() => codec.rowPatchFields(component, "rows", maxSlots, {}));
  assert.throws(() =>
    codec.rowPatchFields(component, "rows", 1, { weight: null }),
  );
  const fields = codec.rowPatchFields(component, "rows", 4, {
    label: null,
    rotation: full.rotation,
    weight: 3,
    count: undefined,
  });
  assert.deepEqual(fields, [
    {
      offset: layout.regionBase + 4 * layout.properties.length,
      value: { kind: "dynamic", value: { kind: "f32", value: 3 } },
    },
    {
      offset: layout.regionBase + 4 * layout.properties.length + 1,
      value: { kind: "dynamic", value: { kind: "vec4", value: [0, 0, 0, 1] } },
    },
    {
      offset: layout.regionBase + 4 * layout.properties.length + 3,
      value: { kind: "unset" },
    },
  ]);
  const commands = codec.rowPatchCommands(
    codec.Entity.handle(7n),
    component,
    "rows",
    4,
    { weight: 3, rotation: full.rotation, label: null },
  );
  assert.deepEqual(
    commands.map((command) => command.field),
    fields,
  );
  full.rotation[0] = 9;
  assert.equal(fields[1].value.value.value[0], 0, "patch owns numeric data");
  assert.deepEqual([...bytes], fixture.rowsExample, "table owns encoded data");
  const huge = {
    nextSlot: 1,
    rows: new Map([
      [0, { weight: 1, source: { kind: 2, source: "a".repeat(1024 * 1024) } }],
    ]),
  };
  assert.throws(() => codec.encodeRowsTable(layout, huge));
  for (const [name, descriptor] of Object.entries(codec.components)) {
    for (const [field, value] of Object.entries(descriptor.fields)) {
      if (!value.rows || value.default === undefined) continue;
      const title = `${field[0].toUpperCase()}${field.slice(1)}`;
      const defaults = new Uint8Array(value.default);
      const table = codec[name][`decode${title}`](defaults);
      assert.deepEqual(
        codec[name][`encode${title}`](table),
        defaults,
        `${target}:${name}.${field} whole table`,
      );
      assert.deepEqual(codec[name][`patch${title}Fields`](0, {}), []);
    }
  }
  assert.deepEqual(codec.GUI_PAINT_PART_KEYS, report[target].paintKeys);
  for (const { index, part, state, variant } of codec.GUI_PAINT_PART_KEYS)
    assert.equal(
      codec.guiPaintPartIndex({
        part,
        ...(state === null ? {} : { state }),
        ...(variant === null ? {} : { variant }),
      }),
      index,
    );
  // Composition is a paint identity that resolves through the label, not a
  // skin part.
  assert.throws(() => codec.guiPaintPartIndex({ part: "composition" }));
  assert.throws(() =>
    codec.guiPaintPartIndex({ part: "fill", variant: "checked" }),
  );
  const paint = {
    nextSlot: 6,
    rows: new Map([
      [
        5,
        {
          part: codec.guiPaintPartIndex({
            part: "fill",
            state: "pressed",
            variant: "checked",
          }),
          opacity: 0.5,
        },
      ],
      [
        2,
        {
          part: codec.guiPaintPartIndex({ part: "background" }),
          color: [0.25, 0.5, 0.75, 1],
          glow_inner_radius: 3,
          corner_cut: [4, 0, 4, 0],
          corner_accent: [0, 8, 0, 8],
          corner_accent_width: 2,
          shape: 1,
          stroke_a: [0.25, 0.5, 0.5, 0.75],
          stroke_b: [0.5, 0.75, 1, 0],
          arc_start: -1.25,
          arc_sweep: 0.75,
          arc_dashes: [48, 0.25],
          fill_mode: 4,
          fill_hue: -0.45,
          checker_size: 6,
          checker_color0: [0.6, 0.6, 0.6, 1],
          checker_color1: [0.3, 0.3, 0.3, 0.5],
        },
      ],
    ]),
  };
  assert.deepEqual(
    [...codec.GuiTheme.encodeParts(paint)],
    fixture.paintExample,
  );
  assert.deepEqual(
    codec.GuiSkin.encodeParts(paint),
    codec.GuiTheme.encodeParts(paint),
  );
  assert.deepEqual(
    codec.GuiTheme.patchPartsFields(2, { opacity: 0.25, color: null }),
    codec.GuiTheme.patchParts(codec.Entity.handle(1n), 2, {
      opacity: 0.25,
      color: null,
    }).map((command) => command.field),
  );
  assert.equal(Object.isFrozen(codec.GUI_PAINT_PART_KEYS), true);
  assert.equal(Object.isFrozen(codec.GUI_PAINT_PART_KEYS[0]), true);

  // The built-in looks are the export's tables, as f32 lanes, and each one is
  // an ordinary GuiTheme.parts table that round-trips the generated codec.
  const f32 = (row) =>
    Object.fromEntries(
      Object.entries(row).map(([key, value]) => [
        key,
        key === "part"
          ? value
          : Array.isArray(value)
            ? value.map(Math.fround)
            : Math.fround(value),
      ]),
    );
  const looks = Object.fromEntries(
    Object.entries(codec.GUI_SKIN_LOOKS).map(([name, look]) => [
      name,
      {
        em: Math.fround(look.em),
        parts: look.parts.map(f32),
        motion: look.motion.map(f32),
      },
    ]),
  );
  assert.deepEqual(looks, report[target].skinLooks);
  for (const name of Object.keys(codec.GUI_SKIN_LOOKS)) {
    const table = codec.guiSkinLookTable(name);
    assert.equal(table.nextSlot, codec.GUI_SKIN_LOOKS[name].parts.length);
    const decoded = codec.GuiTheme.decodeParts(
      codec.GuiTheme.encodeParts(table),
    );
    assert.deepEqual(
      [...decoded.rows.values()].map(f32),
      looks[name].parts,
      `${target}: ${name} look`,
    );
    // Its motion rows are an ordinary GuiThemeMotion.parts table too.
    const motion = codec.guiSkinLookMotionTable(name);
    assert.equal(motion.nextSlot, codec.GUI_SKIN_LOOKS[name].motion.length);
    const timing = codec.GuiThemeMotion.decodeParts(
      codec.GuiThemeMotion.encodeParts(motion),
    );
    assert.deepEqual(
      [...timing.rows.values()].map(f32),
      looks[name].motion,
      `${target}: ${name} motion`,
    );
  }
  assert.equal(Object.isFrozen(codec.GUI_SKIN_LOOKS.switch), true);

  // The design-language tokens are the export's values as f32 lanes.
  const tokens = Object.fromEntries(
    Object.entries(codec.GUI_SKIN_TOKENS).map(([name, value]) => [
      name,
      Array.isArray(value) ? value.map(Math.fround) : Math.fround(value),
    ]),
  );
  assert.deepEqual(tokens, report[target].skinTokens);
  assert.equal(Object.isFrozen(codec.GUI_SKIN_TOKENS), true);
  assert.equal(Object.isFrozen(codec.GUI_SKIN_TOKENS.accent), true);
  const types = `
import { encodeRowsTable, rowPatchFields, GuiTheme, GuiSkin, guiPaintPartIndex, guiSkinLookTable, type RowsInput, type GuiThemePartsRow, type GuiThemePartsRowPatch, type GuiPaintPartKey, type GuiPaintBasePartKey, type GuiSkinLookName } from "./${target}.js";
type Assert<Actual extends true> = Actual;
export type GenericCheck = Assert<{ readonly nextSlot: number; readonly rows: ReadonlyMap<number, Readonly<{ weight: number }>> } extends RowsInput<{ weight: number }> ? true : false>;
export const helpers = [encodeRowsTable, rowPatchFields];
export type GuiChecks = [
  Assert<{} extends GuiThemePartsRow ? false : true>,
  Assert<{ part: number; color: null } extends GuiThemePartsRow ? false : true>,
  Assert<{ color: null } extends GuiThemePartsRowPatch ? true : false>,
  Assert<{ part: null } extends GuiThemePartsRowPatch ? false : true>,
  Assert<{ part: "fill"; variant: "checked" } extends GuiPaintPartKey ? false : true>,
  Assert<{ part: "fill"; state: "pressed"; variant: "checked" } extends GuiPaintPartKey ? true : false>,
  Assert<{ part: "fill"; state: "pressed" } extends GuiPaintBasePartKey ? false : true>,
  Assert<{ part: "fill" } extends GuiPaintBasePartKey ? true : false>,
  Assert<{ part: number; glow_inner_radius: number; corner_cut: readonly [number, number, number, number]; corner_accent: readonly [number, number, number, number]; corner_accent_width: number; shape: number; stroke_a: readonly [number, number, number, number]; stroke_b: readonly [number, number, number, number]; arc_start: number; arc_sweep: number; arc_dashes: readonly [number, number]; fill_hue: number; checker_size: number; checker_color0: readonly [number, number, number, number]; checker_color1: readonly [number, number, number, number] } extends GuiThemePartsRow ? true : false>,
  Assert<{ part: number; corner_cut: readonly [number, number] } extends GuiThemePartsRow ? false : true>,
  Assert<{ part: number; arc_dashes: readonly [number, number, number, number] } extends GuiThemePartsRow ? false : true>,
  Assert<{ part: number; checker_color0: readonly [number, number] } extends GuiThemePartsRow ? false : true>,
  Assert<{ stroke_a: null; corner_accent: null; arc_sweep: null; fill_hue: null; checker_size: null } extends GuiThemePartsRowPatch ? true : false>,
  Assert<"switch" extends GuiSkinLookName ? true : false>,
  Assert<"amber" extends GuiSkinLookName ? true : false>,
  Assert<"unknown" extends GuiSkinLookName ? false : true>
];
export const switchTheme = GuiTheme.encodeParts(guiSkinLookTable("switch"));
const parts: RowsInput<GuiThemePartsRow> = { nextSlot: 4, rows: new Map([[3, { part: guiPaintPartIndex({ part: "background" }), color: [1, 0, 0, 1] }]]) };
export const encoded = GuiTheme.encodeParts(parts);
export const decoded = GuiTheme.decodeParts(encoded);
export const patch = GuiSkin.patchPartsFields(3, { color: null, opacity: 0.5 });
`;
  await writeFile(path.join(directory, `${target}-rows-types.ts`), types);
  results.push({
    target,
    hash: codec.SCHEMA_HASH.toString(),
    rowsBytes: bytes.length,
    paintKeys: report[target].paintKeys.length,
  });
}
await writeFile(
  path.join(directory, "rows-report.json"),
  `${JSON.stringify(results, null, 2)}\n`,
);
console.log(
  "Generic rows helpers match native/executed-WASM core bytes, detached patches, and compiled GUI keys.",
);
